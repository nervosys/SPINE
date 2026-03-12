//! # Chaos Engineering Framework
//!
//! Automated fault injection for testing agent-level resilience.
//! Runs campaigns that inject faults (message drops, delays, agent crashes,
//! network partitions) and verifies that the system degrades gracefully.
//!
//! ## Features
//!
//! - **FaultType**: 10 fault variants covering network, agent, and resource failures
//! - **ChaosScenario**: Named, repeatable fault injection plans
//! - **FaultInjector**: Applies faults and tracks injection state
//! - **CampaignRunner**: Executes multi-step campaigns with per-step verification
//! - **CampaignReport**: Summary of injected faults, observed anomalies, and verdicts

use crate::anomaly::{Anomaly, AnomalyDetector, DetectorConfig, MetricSample};
use crate::AgentId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt::Write;
use uuid::Uuid;

/// Encode bytes as lowercase hex string.
fn encode_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

// ──────────────────────────── Fault Types ────────────────────────────

/// A fault that can be injected into the system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FaultType {
    /// Drop messages with given probability (0.0–1.0)
    MessageDrop { probability: f64 },
    /// Delay messages by a fixed duration (milliseconds)
    MessageDelay { delay_ms: u64 },
    /// Corrupt message payload bytes (flip random bits)
    MessageCorruption { corruption_rate: f64 },
    /// Crash an agent (simulate process death)
    AgentCrash { agent_id: AgentId },
    /// Hang an agent (simulate infinite loop / deadlock)
    AgentHang { agent_id: AgentId, duration_ms: u64 },
    /// Partition a set of agents from the rest of the mesh
    NetworkPartition { isolated: Vec<AgentId> },
    /// Exhaust memory budget for an agent
    ResourceExhaustion { agent_id: AgentId, memory_bytes: u64 },
    /// Inject clock skew (milliseconds ahead/behind)
    ClockSkew { agent_id: AgentId, skew_ms: i64 },
    /// Duplicate messages (replay attack simulation)
    MessageDuplicate { duplication_factor: u32 },
    /// Throttle bandwidth between agents (bytes per second)
    BandwidthThrottle { limit_bps: u64 },
}

impl FaultType {
    /// Human-readable category name for this fault.
    pub fn category(&self) -> &'static str {
        match self {
            FaultType::MessageDrop { .. } => "message_drop",
            FaultType::MessageDelay { .. } => "message_delay",
            FaultType::MessageCorruption { .. } => "message_corruption",
            FaultType::AgentCrash { .. } => "agent_crash",
            FaultType::AgentHang { .. } => "agent_hang",
            FaultType::NetworkPartition { .. } => "network_partition",
            FaultType::ResourceExhaustion { .. } => "resource_exhaustion",
            FaultType::ClockSkew { .. } => "clock_skew",
            FaultType::MessageDuplicate { .. } => "message_duplicate",
            FaultType::BandwidthThrottle { .. } => "bandwidth_throttle",
        }
    }
}

// ──────────────────────────── Scenarios ────────────────────────────

/// A named, repeatable chaos scenario comprising ordered fault injection steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosScenario {
    /// Unique identifier
    pub id: Uuid,
    /// Human-readable name
    pub name: String,
    /// Description of what this scenario tests
    pub description: String,
    /// Ordered steps to execute
    pub steps: Vec<ChaosStep>,
    /// SHA-256 hash of the scenario definition for integrity verification
    pub hash: String,
}

/// A single step in a chaos scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosStep {
    /// Step index (0-based)
    pub index: usize,
    /// Fault to inject
    pub fault: FaultType,
    /// Duration to hold the fault active (milliseconds)
    pub duration_ms: u64,
    /// Whether the system should recover after this step
    pub expect_recovery: bool,
    /// Optional verification: expected anomaly types that should be detected
    pub expected_anomalies: Vec<String>,
}

impl ChaosScenario {
    /// Create a new scenario with the given name and compute its hash.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        let mut s = Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: description.into(),
            steps: Vec::new(),
            hash: String::new(),
        };
        s.rehash();
        s
    }

    /// Add a step and rehash.
    pub fn add_step(&mut self, fault: FaultType, duration_ms: u64, expect_recovery: bool) {
        let step = ChaosStep {
            index: self.steps.len(),
            fault,
            duration_ms,
            expect_recovery,
            expected_anomalies: Vec::new(),
        };
        self.steps.push(step);
        self.rehash();
    }

    /// Add a step with expected anomaly annotations.
    pub fn add_step_with_anomalies(
        &mut self,
        fault: FaultType,
        duration_ms: u64,
        expect_recovery: bool,
        expected_anomalies: Vec<String>,
    ) {
        let step = ChaosStep {
            index: self.steps.len(),
            fault,
            duration_ms,
            expect_recovery,
            expected_anomalies,
        };
        self.steps.push(step);
        self.rehash();
    }

    /// Recompute the integrity hash.
    fn rehash(&mut self) {
        let mut hasher = Sha256::new();
        hasher.update(self.name.as_bytes());
        hasher.update(self.description.as_bytes());
        for step in &self.steps {
            hasher.update(step.index.to_le_bytes());
            hasher.update(step.duration_ms.to_le_bytes());
            hasher.update(step.fault.category().as_bytes());
        }
        self.hash = encode_hex(&hasher.finalize());
    }

    /// Verify scenario integrity.
    pub fn verify_integrity(&self) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(self.name.as_bytes());
        hasher.update(self.description.as_bytes());
        for step in &self.steps {
            hasher.update(step.index.to_le_bytes());
            hasher.update(step.duration_ms.to_le_bytes());
            hasher.update(step.fault.category().as_bytes());
        }
        encode_hex(&hasher.finalize()) == self.hash
    }

    /// Number of steps.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

// ──────────────────────── Predefined Scenarios ────────────────────────

impl ChaosScenario {
    /// Rolling agent crashes: each agent crashes sequentially.
    pub fn rolling_crashes(agents: &[AgentId]) -> Self {
        let mut s = Self::new(
            "rolling_crashes",
            "Sequential agent crashes to test failover and recovery",
        );
        for agent_id in agents {
            s.add_step_with_anomalies(
                FaultType::AgentCrash {
                    agent_id: *agent_id,
                },
                2000,
                true,
                vec!["spike".to_string()],
            );
        }
        s
    }

    /// Network split: partition agents into two halves.
    pub fn network_split(agents: &[AgentId]) -> Self {
        let half = agents.len() / 2;
        let isolated: Vec<AgentId> = agents[..half].to_vec();
        let mut s = Self::new(
            "network_split",
            "Partition agents to test split-brain handling",
        );
        s.add_step_with_anomalies(
            FaultType::NetworkPartition { isolated },
            5000,
            true,
            vec!["drift".to_string()],
        );
        s
    }

    /// Gradual degradation: increasing message drop rates.
    pub fn gradual_degradation() -> Self {
        let mut s = Self::new(
            "gradual_degradation",
            "Increasing message loss to test adaptive behavior",
        );
        for &p in &[0.1, 0.3, 0.5, 0.7, 0.9] {
            s.add_step(FaultType::MessageDrop { probability: p }, 3000, true);
        }
        s
    }

    /// Combined faults: multiple fault types simultaneously.
    pub fn combined_faults(target: AgentId) -> Self {
        let mut s = Self::new(
            "combined_faults",
            "Multiple simultaneous faults to test compound resilience",
        );
        s.add_step(FaultType::MessageDelay { delay_ms: 500 }, 2000, true);
        s.add_step(FaultType::MessageCorruption { corruption_rate: 0.1 }, 2000, true);
        s.add_step(
            FaultType::AgentHang {
                agent_id: target,
                duration_ms: 3000,
            },
            3000,
            true,
        );
        s
    }
}

// ──────────────────────────── Fault Injector ────────────────────────────

/// Tracks active faults and determines whether a given operation should be affected.
#[derive(Debug)]
pub struct FaultInjector {
    /// Currently active faults with their activation timestamp
    active_faults: Vec<ActiveFault>,
    /// Total faults injected across all campaigns
    total_injected: u64,
    /// Fault injection log for audit
    log: Vec<InjectionEvent>,
}

/// An active fault with its expiry.
#[derive(Debug, Clone)]
struct ActiveFault {
    fault: FaultType,
    activated_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    scenario_id: Uuid,
    step_index: usize,
}

/// A logged injection event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionEvent {
    pub timestamp: DateTime<Utc>,
    pub fault_category: String,
    pub scenario_id: Uuid,
    pub step_index: usize,
    pub action: InjectionAction,
}

/// Whether a fault was activated or deactivated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InjectionAction {
    Activated,
    Deactivated,
    Expired,
}

impl FaultInjector {
    /// Create a new fault injector.
    pub fn new() -> Self {
        Self {
            active_faults: Vec::new(),
            total_injected: 0,
            log: Vec::new(),
        }
    }

    /// Activate a fault for the given duration.
    pub fn inject(&mut self, fault: FaultType, duration_ms: u64, scenario_id: Uuid, step_index: usize) {
        let now = Utc::now();
        let expires = now + chrono::Duration::milliseconds(duration_ms as i64);
        self.active_faults.push(ActiveFault {
            fault: fault.clone(),
            activated_at: now,
            expires_at: expires,
            scenario_id,
            step_index,
        });
        self.total_injected += 1;
        self.log.push(InjectionEvent {
            timestamp: now,
            fault_category: fault.category().to_string(),
            scenario_id,
            step_index,
            action: InjectionAction::Activated,
        });
    }

    /// Remove expired faults and log their expiry.
    pub fn tick(&mut self) {
        let now = Utc::now();
        let (expired, active): (Vec<_>, Vec<_>) = self
            .active_faults
            .drain(..)
            .partition(|f| f.expires_at <= now);
        self.active_faults = active;
        for e in expired {
            self.log.push(InjectionEvent {
                timestamp: now,
                fault_category: e.fault.category().to_string(),
                scenario_id: e.scenario_id,
                step_index: e.step_index,
                action: InjectionAction::Expired,
            });
        }
    }

    /// Deactivate all active faults.
    pub fn clear(&mut self) {
        let now = Utc::now();
        for f in self.active_faults.drain(..) {
            self.log.push(InjectionEvent {
                timestamp: now,
                fault_category: f.fault.category().to_string(),
                scenario_id: f.scenario_id,
                step_index: f.step_index,
                action: InjectionAction::Deactivated,
            });
        }
    }

    /// Check whether a message to/from the given agent should be dropped.
    pub fn should_drop_message(&self, _agent: &AgentId) -> bool {
        for af in &self.active_faults {
            if let FaultType::MessageDrop { probability } = af.fault {
                // Deterministic for testability: drop if probability >= 0.5
                // In production use, wire this to a CSPRNG
                if probability >= 0.5 {
                    return true;
                }
            }
        }
        false
    }

    /// Check whether a message should be delayed, returning the delay in ms.
    pub fn message_delay_ms(&self) -> u64 {
        self.active_faults
            .iter()
            .filter_map(|af| match af.fault {
                FaultType::MessageDelay { delay_ms } => Some(delay_ms),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// Check whether the given agent is crashed.
    pub fn is_agent_crashed(&self, agent: &AgentId) -> bool {
        self.active_faults.iter().any(|af| matches!(&af.fault, FaultType::AgentCrash { agent_id } if agent_id == agent))
    }

    /// Check whether the given agent is hanging.
    pub fn is_agent_hanging(&self, agent: &AgentId) -> bool {
        self.active_faults.iter().any(|af| matches!(&af.fault, FaultType::AgentHang { agent_id, .. } if agent_id == agent))
    }

    /// Check whether the given agent is in an isolated partition.
    pub fn is_partitioned(&self, agent: &AgentId) -> bool {
        self.active_faults.iter().any(|af| {
            matches!(&af.fault, FaultType::NetworkPartition { isolated } if isolated.contains(agent))
        })
    }

    /// Number of currently active faults.
    pub fn active_count(&self) -> usize {
        self.active_faults.len()
    }

    /// Total faults injected.
    pub fn total_injected(&self) -> u64 {
        self.total_injected
    }

    /// Get the injection log.
    pub fn log(&self) -> &[InjectionEvent] {
        &self.log
    }
}

impl Default for FaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────── Campaign Runner ────────────────────────────

/// Verdict for a single campaign step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepVerdict {
    /// Step completed and system behaved as expected
    Pass,
    /// Step completed but anomalies didn't match expectations
    Fail { reason: String },
    /// Step could not be executed
    Skipped { reason: String },
}

/// Result of a single executed step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step_index: usize,
    pub fault_category: String,
    pub verdict: StepVerdict,
    pub anomalies_detected: Vec<String>,
    pub duration_ms: u64,
}

/// Final report for a completed campaign.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignReport {
    pub scenario_name: String,
    pub scenario_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub step_results: Vec<StepResult>,
    pub total_faults_injected: u64,
    pub total_anomalies: usize,
    pub pass_count: usize,
    pub fail_count: usize,
    pub overall_verdict: OverallVerdict,
}

/// Overall campaign result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OverallVerdict {
    /// All steps passed
    Pass,
    /// Some steps failed
    PartialFailure,
    /// Majority of steps failed
    Failure,
}

/// Runs chaos campaigns by executing scenarios step-by-step.
pub struct CampaignRunner {
    injector: FaultInjector,
    detector: AnomalyDetector,
    reports: Vec<CampaignReport>,
}

impl CampaignRunner {
    /// Create a campaign runner with default anomaly detection config.
    pub fn new() -> Self {
        Self {
            injector: FaultInjector::new(),
            detector: AnomalyDetector::new(DetectorConfig::default(), 100),
            reports: Vec::new(),
        }
    }

    /// Create with a custom anomaly detector config.
    pub fn with_detector_config(config: DetectorConfig, window_size: usize) -> Self {
        Self {
            injector: FaultInjector::new(),
            detector: AnomalyDetector::new(config, window_size),
            reports: Vec::new(),
        }
    }

    /// Execute a chaos scenario, collecting metrics via the provided sampler.
    ///
    /// The `sampler` closure is called after each fault injection step to collect
    /// system metrics. It receives the step index and returns metric samples.
    pub fn run_scenario<F>(&mut self, scenario: &ChaosScenario, mut sampler: F) -> CampaignReport
    where
        F: FnMut(usize) -> Vec<MetricSample>,
    {
        let started_at = Utc::now();
        let mut step_results = Vec::new();

        for step in &scenario.steps {
            // Inject the fault
            self.injector.inject(
                step.fault.clone(),
                step.duration_ms,
                scenario.id,
                step.index,
            );

            // Collect metrics from the sampler
            let samples = sampler(step.index);

            // Feed metrics to the anomaly detector
            let mut detected_anomalies: Vec<Anomaly> = Vec::new();
            for sample in samples {
                let anomalies = self.detector.ingest(sample);
                detected_anomalies.extend(anomalies);
            }

            let anomaly_types: Vec<String> = detected_anomalies
                .iter()
                .map(|a| format!("{:?}", a.anomaly_type))
                .collect();

            // Determine step verdict
            let verdict = self.evaluate_step(step, &anomaly_types);

            step_results.push(StepResult {
                step_index: step.index,
                fault_category: step.fault.category().to_string(),
                verdict,
                anomalies_detected: anomaly_types,
                duration_ms: step.duration_ms,
            });

            // Expire the fault
            self.injector.tick();
        }

        // Clear remaining faults
        self.injector.clear();

        let pass_count = step_results
            .iter()
            .filter(|r| r.verdict == StepVerdict::Pass)
            .count();
        let fail_count = step_results
            .iter()
            .filter(|r| matches!(r.verdict, StepVerdict::Fail { .. }))
            .count();
        let total_anomalies: usize = step_results
            .iter()
            .map(|r| r.anomalies_detected.len())
            .sum();

        let overall_verdict = if fail_count == 0 {
            OverallVerdict::Pass
        } else if fail_count <= step_results.len() / 2 {
            OverallVerdict::PartialFailure
        } else {
            OverallVerdict::Failure
        };

        let report = CampaignReport {
            scenario_name: scenario.name.clone(),
            scenario_id: scenario.id,
            started_at,
            completed_at: Utc::now(),
            step_results,
            total_faults_injected: self.injector.total_injected(),
            total_anomalies,
            pass_count,
            fail_count,
            overall_verdict,
        };

        self.reports.push(report.clone());
        report
    }

    /// Evaluate a step's result against expectations.
    fn evaluate_step(&self, step: &ChaosStep, detected: &[String]) -> StepVerdict {
        if step.expected_anomalies.is_empty() {
            // No expectations → pass
            return StepVerdict::Pass;
        }

        let expected: HashSet<&str> = step
            .expected_anomalies
            .iter()
            .map(String::as_str)
            .collect();
        let found: HashSet<&str> = detected.iter().map(String::as_str).collect();

        let missing: Vec<&&str> = expected.difference(&found).collect();
        if missing.is_empty() {
            StepVerdict::Pass
        } else {
            StepVerdict::Fail {
                reason: format!(
                    "Expected anomalies not detected: {:?}",
                    missing.iter().map(|s| s.to_string()).collect::<Vec<_>>()
                ),
            }
        }
    }

    /// Get all past campaign reports.
    pub fn reports(&self) -> &[CampaignReport] {
        &self.reports
    }

    /// Get the fault injector (for querying active faults).
    pub fn injector(&self) -> &FaultInjector {
        &self.injector
    }

    /// Get a mutable reference to the injector.
    pub fn injector_mut(&mut self) -> &mut FaultInjector {
        &mut self.injector
    }
}

impl Default for CampaignRunner {
    fn default() -> Self {
        Self::new()
    }
}

// ══════════════════════════════ Tests ══════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn test_agent(n: u8) -> AgentId {
        AgentId(Uuid::from_bytes([n; 16]))
    }

    // ── FaultType ──

    #[test]
    fn test_fault_type_categories() {
        assert_eq!(
            FaultType::MessageDrop { probability: 0.5 }.category(),
            "message_drop"
        );
        assert_eq!(
            FaultType::AgentCrash { agent_id: test_agent(1) }.category(),
            "agent_crash"
        );
        assert_eq!(
            FaultType::NetworkPartition { isolated: vec![] }.category(),
            "network_partition"
        );
        assert_eq!(
            FaultType::BandwidthThrottle { limit_bps: 1000 }.category(),
            "bandwidth_throttle"
        );
    }

    // ── ChaosScenario ──

    #[test]
    fn test_scenario_integrity() {
        let mut scenario = ChaosScenario::new("test", "A test scenario");
        assert!(scenario.verify_integrity());
        scenario.add_step(FaultType::MessageDrop { probability: 0.1 }, 1000, true);
        assert!(scenario.verify_integrity());
        assert_eq!(scenario.step_count(), 1);
    }

    #[test]
    fn test_scenario_integrity_tampered() {
        let mut scenario = ChaosScenario::new("test", "A test scenario");
        scenario.add_step(FaultType::MessageDrop { probability: 0.1 }, 1000, true);
        // Tamper with the name after hashing
        scenario.name = "tampered".to_string();
        assert!(!scenario.verify_integrity());
    }

    #[test]
    fn test_predefined_rolling_crashes() {
        let agents = vec![test_agent(1), test_agent(2), test_agent(3)];
        let scenario = ChaosScenario::rolling_crashes(&agents);
        assert_eq!(scenario.step_count(), 3);
        assert!(scenario.verify_integrity());
    }

    #[test]
    fn test_predefined_network_split() {
        let agents = vec![test_agent(1), test_agent(2), test_agent(3), test_agent(4)];
        let scenario = ChaosScenario::network_split(&agents);
        assert_eq!(scenario.step_count(), 1);
        assert!(scenario.verify_integrity());
    }

    #[test]
    fn test_predefined_gradual_degradation() {
        let scenario = ChaosScenario::gradual_degradation();
        assert_eq!(scenario.step_count(), 5);
        assert!(scenario.verify_integrity());
    }

    #[test]
    fn test_predefined_combined_faults() {
        let scenario = ChaosScenario::combined_faults(test_agent(1));
        assert_eq!(scenario.step_count(), 3);
        assert!(scenario.verify_integrity());
    }

    // ── FaultInjector ──

    #[test]
    fn test_injector_inject_and_count() {
        let mut injector = FaultInjector::new();
        assert_eq!(injector.active_count(), 0);
        assert_eq!(injector.total_injected(), 0);

        let sid = Uuid::new_v4();
        injector.inject(FaultType::MessageDrop { probability: 0.5 }, 60_000, sid, 0);
        assert_eq!(injector.active_count(), 1);
        assert_eq!(injector.total_injected(), 1);
    }

    #[test]
    fn test_injector_should_drop() {
        let mut injector = FaultInjector::new();
        let agent = test_agent(1);
        let sid = Uuid::new_v4();

        // Low probability: no drop
        injector.inject(FaultType::MessageDrop { probability: 0.1 }, 60_000, sid, 0);
        assert!(!injector.should_drop_message(&agent));

        injector.clear();

        // High probability: drop
        injector.inject(FaultType::MessageDrop { probability: 0.8 }, 60_000, sid, 1);
        assert!(injector.should_drop_message(&agent));
    }

    #[test]
    fn test_injector_message_delay() {
        let mut injector = FaultInjector::new();
        let sid = Uuid::new_v4();
        assert_eq!(injector.message_delay_ms(), 0);

        injector.inject(FaultType::MessageDelay { delay_ms: 200 }, 60_000, sid, 0);
        assert_eq!(injector.message_delay_ms(), 200);

        injector.inject(FaultType::MessageDelay { delay_ms: 500 }, 60_000, sid, 1);
        assert_eq!(injector.message_delay_ms(), 500); // max of active delays
    }

    #[test]
    fn test_injector_agent_state_checks() {
        let mut injector = FaultInjector::new();
        let agent = test_agent(1);
        let other = test_agent(2);
        let sid = Uuid::new_v4();

        injector.inject(
            FaultType::AgentCrash { agent_id: agent.clone() },
            60_000,
            sid,
            0,
        );
        assert!(injector.is_agent_crashed(&agent));
        assert!(!injector.is_agent_crashed(&other));
        assert!(!injector.is_agent_hanging(&agent));
    }

    #[test]
    fn test_injector_partition_check() {
        let mut injector = FaultInjector::new();
        let a1 = test_agent(1);
        let a2 = test_agent(2);
        let a3 = test_agent(3);
        let sid = Uuid::new_v4();

        injector.inject(
            FaultType::NetworkPartition {
                isolated: vec![a1.clone(), a2.clone()],
            },
            60_000,
            sid,
            0,
        );
        assert!(injector.is_partitioned(&a1));
        assert!(injector.is_partitioned(&a2));
        assert!(!injector.is_partitioned(&a3));
    }

    #[test]
    fn test_injector_clear_logs_deactivation() {
        let mut injector = FaultInjector::new();
        let sid = Uuid::new_v4();
        injector.inject(FaultType::MessageDrop { probability: 0.5 }, 60_000, sid, 0);
        injector.inject(FaultType::MessageDelay { delay_ms: 100 }, 60_000, sid, 1);
        assert_eq!(injector.active_count(), 2);

        injector.clear();
        assert_eq!(injector.active_count(), 0);

        // Log should have 2 activated + 2 deactivated = 4 entries
        assert_eq!(injector.log().len(), 4);
        let deactivated = injector
            .log()
            .iter()
            .filter(|e| e.action == InjectionAction::Deactivated)
            .count();
        assert_eq!(deactivated, 2);
    }

    // ── CampaignRunner ──

    #[test]
    fn test_campaign_runner_empty_scenario() {
        let mut runner = CampaignRunner::new();
        let scenario = ChaosScenario::new("empty", "Nothing to do");
        let report = runner.run_scenario(&scenario, |_| Vec::new());
        assert_eq!(report.step_results.len(), 0);
        assert_eq!(report.overall_verdict, OverallVerdict::Pass);
    }

    #[test]
    fn test_campaign_runner_pass_no_expectations() {
        let mut runner = CampaignRunner::new();
        let mut scenario = ChaosScenario::new("simple", "Simple drop test");
        scenario.add_step(FaultType::MessageDrop { probability: 0.5 }, 100, true);

        let report = runner.run_scenario(&scenario, |_| Vec::new());
        assert_eq!(report.step_results.len(), 1);
        assert_eq!(report.step_results[0].verdict, StepVerdict::Pass);
        assert_eq!(report.overall_verdict, OverallVerdict::Pass);
    }

    #[test]
    fn test_campaign_runner_with_sampled_metrics() {
        let mut runner = CampaignRunner::new();
        let mut scenario = ChaosScenario::new("metrics", "Metrics fed scenario");
        scenario.add_step(FaultType::MessageDelay { delay_ms: 200 }, 100, true);

        let report = runner.run_scenario(&scenario, |step_idx| {
            vec![MetricSample {
                agent_id: test_agent(1),
                timestamp: Utc::now(),
                latency_ms: 100.0 + (step_idx as f64) * 50.0,
                throughput: 1000.0,
                error_rate: 0.0,
                queue_depth: 5,
                progress: 0.8,
            }]
        });

        assert_eq!(report.pass_count, 1);
        assert_eq!(report.fail_count, 0);
    }

    #[test]
    fn test_campaign_runner_reports_persisted() {
        let mut runner = CampaignRunner::new();
        let scenario = ChaosScenario::new("persist", "Test report storage");
        runner.run_scenario(&scenario, |_| Vec::new());
        runner.run_scenario(&scenario, |_| Vec::new());
        assert_eq!(runner.reports().len(), 2);
    }

    #[test]
    fn test_campaign_runner_fail_missing_anomaly() {
        let mut runner = CampaignRunner::new();
        let mut scenario = ChaosScenario::new("fail", "Expected anomaly missing");
        scenario.add_step_with_anomalies(
            FaultType::MessageDrop { probability: 0.5 },
            100,
            true,
            vec!["spike".to_string()], // Expect a spike anomaly
        );

        // No metrics → no anomalies detected → should fail
        let report = runner.run_scenario(&scenario, |_| Vec::new());
        assert_eq!(report.fail_count, 1);
        assert!(matches!(
            report.overall_verdict,
            OverallVerdict::Failure | OverallVerdict::PartialFailure
        ));
    }

    #[test]
    fn test_default_impls() {
        let _injector = FaultInjector::default();
        let _runner = CampaignRunner::default();
    }

    #[test]
    fn test_step_with_anomalies() {
        let mut scenario = ChaosScenario::new("annotated", "Annotated scenario");
        scenario.add_step_with_anomalies(
            FaultType::AgentCrash { agent_id: test_agent(1) },
            1000,
            true,
            vec!["spike".to_string(), "drift".to_string()],
        );
        assert_eq!(scenario.steps[0].expected_anomalies.len(), 2);
    }
}
