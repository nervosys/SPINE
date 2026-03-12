// =============================================================================
// ANOMALY: Behavioral Anomaly Detection for Agent Swarms
// =============================================================================
//
// Detects drift (gradual performance degradation), deadlock (agents stuck
// waiting), and livelock (agents busy but making no progress) through
// statistical monitoring of agent metrics over sliding windows.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::AgentId;

/// An anomaly detected in the swarm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    pub id: Uuid,
    pub detected_at: DateTime<Utc>,
    pub anomaly_type: AnomalyType,
    pub severity: Severity,
    pub affected_agents: Vec<AgentId>,
    pub description: String,
    pub evidence: Vec<Evidence>,
}

/// Classification of anomaly.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AnomalyType {
    /// Gradual performance degradation over time.
    Drift,
    /// Agent(s) blocked waiting for each other.
    Deadlock,
    /// Agents busy but making no meaningful progress.
    Livelock,
    /// Sudden spike or drop in a metric.
    Spike,
    /// Agent unreachable or unresponsive.
    Unresponsive,
}

/// Severity level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// Evidence supporting an anomaly detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub metric: String,
    pub expected: f64,
    pub observed: f64,
    pub timestamp: DateTime<Utc>,
}

/// A metric sample for an agent.
#[derive(Debug, Clone)]
pub struct MetricSample {
    pub agent_id: AgentId,
    pub timestamp: DateTime<Utc>,
    pub latency_ms: f64,
    pub throughput: f64,
    pub error_rate: f64,
    pub progress: f64,
    pub queue_depth: u64,
}

/// Detects anomalies in agent behavior using sliding windows.
pub struct AnomalyDetector {
    /// Per-agent sliding window of samples.
    windows: HashMap<AgentId, Vec<MetricSample>>,
    /// Max samples per agent.
    window_size: usize,
    /// Detected anomalies.
    anomalies: Vec<Anomaly>,
    /// Thresholds.
    config: DetectorConfig,
}

/// Configuration for the anomaly detector.
#[derive(Debug, Clone)]
pub struct DetectorConfig {
    /// Standard deviations for spike detection.
    pub spike_sigma: f64,
    /// Min samples before drift detection.
    pub drift_min_samples: usize,
    /// Slope threshold for drift (negative = degradation).
    pub drift_slope_threshold: f64,
    /// Progress below this for `livelock_window` samples triggers livelock.
    pub livelock_progress_threshold: f64,
    /// Consecutive low-progress samples for livelock.
    pub livelock_window: usize,
    /// Queue depth above this + zero throughput = potential deadlock.
    pub deadlock_queue_threshold: u64,
    /// Consecutive zero-throughput samples for deadlock.
    pub deadlock_window: usize,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            spike_sigma: 3.0,
            drift_min_samples: 10,
            drift_slope_threshold: -0.1,
            livelock_progress_threshold: 0.01,
            livelock_window: 5,
            deadlock_queue_threshold: 10,
            deadlock_window: 3,
        }
    }
}

impl AnomalyDetector {
    pub fn new(config: DetectorConfig, window_size: usize) -> Self {
        Self {
            windows: HashMap::new(),
            window_size,
            anomalies: Vec::new(),
            config,
        }
    }

    /// Ingest a metric sample and check for anomalies.
    pub fn ingest(&mut self, sample: MetricSample) -> Vec<Anomaly> {
        let agent_id = sample.agent_id;
        let window = self.windows.entry(agent_id).or_default();

        if window.len() >= self.window_size {
            window.remove(0);
        }
        window.push(sample);

        let mut found = Vec::new();

        if let Some(a) = self.detect_spike(agent_id) {
            found.push(a);
        }
        if let Some(a) = self.detect_drift(agent_id) {
            found.push(a);
        }
        if let Some(a) = self.detect_livelock(agent_id) {
            found.push(a);
        }
        if let Some(a) = self.detect_deadlock(agent_id) {
            found.push(a);
        }

        self.anomalies.extend(found.clone());
        found
    }

    /// Get all detected anomalies.
    pub fn anomalies(&self) -> &[Anomaly] {
        &self.anomalies
    }

    /// Get anomalies by type.
    pub fn anomalies_by_type(&self, t: AnomalyType) -> Vec<&Anomaly> {
        self.anomalies
            .iter()
            .filter(|a| a.anomaly_type == t)
            .collect()
    }

    /// Get anomalies by severity at or above threshold.
    pub fn anomalies_above_severity(&self, min: Severity) -> Vec<&Anomaly> {
        self.anomalies
            .iter()
            .filter(|a| a.severity >= min)
            .collect()
    }

    /// Clear all anomalies.
    pub fn clear(&mut self) {
        self.anomalies.clear();
    }

    /// Clear windows for an agent.
    pub fn clear_agent(&mut self, agent_id: AgentId) {
        self.windows.remove(&agent_id);
    }

    // ---- Detectors ----

    fn detect_spike(&self, agent_id: AgentId) -> Option<Anomaly> {
        let window = self.windows.get(&agent_id)?;
        if window.len() < 3 {
            return None;
        }

        let latencies: Vec<f64> = window.iter().map(|s| s.latency_ms).collect();
        let n = latencies.len();
        let latest = latencies[n - 1];

        // Mean and stddev of all except latest
        let prior = &latencies[..n - 1];
        let mean = prior.iter().sum::<f64>() / prior.len() as f64;
        let variance = prior.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / prior.len() as f64;
        let stddev = variance.sqrt();

        if stddev > 0.0 && (latest - mean).abs() > self.config.spike_sigma * stddev {
            let severity = if (latest - mean).abs() > 5.0 * stddev {
                Severity::Critical
            } else if (latest - mean).abs() > 4.0 * stddev {
                Severity::High
            } else {
                Severity::Medium
            };

            Some(Anomaly {
                id: Uuid::new_v4(),
                detected_at: Utc::now(),
                anomaly_type: AnomalyType::Spike,
                severity,
                affected_agents: vec![agent_id],
                description: format!(
                    "Latency spike: {latest:.1}ms vs mean {mean:.1}ms ({:.1}σ)",
                    (latest - mean) / stddev
                ),
                evidence: vec![Evidence {
                    metric: "latency_ms".into(),
                    expected: mean,
                    observed: latest,
                    timestamp: window.last().unwrap().timestamp,
                }],
            })
        } else {
            None
        }
    }

    fn detect_drift(&self, agent_id: AgentId) -> Option<Anomaly> {
        let window = self.windows.get(&agent_id)?;
        if window.len() < self.config.drift_min_samples {
            return None;
        }

        // Linear regression on throughput
        let n = window.len() as f64;
        let throughputs: Vec<f64> = window.iter().map(|s| s.throughput).collect();

        let x_mean = (n - 1.0) / 2.0;
        let y_mean = throughputs.iter().sum::<f64>() / n;

        let mut num = 0.0;
        let mut den = 0.0;
        for (i, &y) in throughputs.iter().enumerate() {
            let x = i as f64;
            num += (x - x_mean) * (y - y_mean);
            den += (x - x_mean).powi(2);
        }

        if den == 0.0 {
            return None;
        }
        let slope = num / den;

        if slope < self.config.drift_slope_threshold {
            Some(Anomaly {
                id: Uuid::new_v4(),
                detected_at: Utc::now(),
                anomaly_type: AnomalyType::Drift,
                severity: if slope < self.config.drift_slope_threshold * 3.0 {
                    Severity::High
                } else {
                    Severity::Medium
                },
                affected_agents: vec![agent_id],
                description: format!("Throughput drift: slope={slope:.4}/sample over {n} samples"),
                evidence: vec![Evidence {
                    metric: "throughput_slope".into(),
                    expected: 0.0,
                    observed: slope,
                    timestamp: window.last().unwrap().timestamp,
                }],
            })
        } else {
            None
        }
    }

    fn detect_livelock(&self, agent_id: AgentId) -> Option<Anomaly> {
        let window = self.windows.get(&agent_id)?;
        if window.len() < self.config.livelock_window {
            return None;
        }

        let tail = &window[window.len() - self.config.livelock_window..];
        let all_low_progress = tail
            .iter()
            .all(|s| s.progress < self.config.livelock_progress_threshold);
        let has_throughput = tail.iter().any(|s| s.throughput > 0.0);

        if all_low_progress && has_throughput {
            Some(Anomaly {
                id: Uuid::new_v4(),
                detected_at: Utc::now(),
                anomaly_type: AnomalyType::Livelock,
                severity: Severity::High,
                affected_agents: vec![agent_id],
                description: format!(
                    "Livelock: {} consecutive samples with progress < {} but non-zero throughput",
                    self.config.livelock_window, self.config.livelock_progress_threshold
                ),
                evidence: tail
                    .iter()
                    .map(|s| Evidence {
                        metric: "progress".into(),
                        expected: self.config.livelock_progress_threshold,
                        observed: s.progress,
                        timestamp: s.timestamp,
                    })
                    .collect(),
            })
        } else {
            None
        }
    }

    fn detect_deadlock(&self, agent_id: AgentId) -> Option<Anomaly> {
        let window = self.windows.get(&agent_id)?;
        if window.len() < self.config.deadlock_window {
            return None;
        }

        let tail = &window[window.len() - self.config.deadlock_window..];
        let all_stuck = tail.iter().all(|s| {
            s.throughput == 0.0 && s.queue_depth >= self.config.deadlock_queue_threshold
        });

        if all_stuck {
            let max_queue = tail.iter().map(|s| s.queue_depth).max().unwrap_or(0);
            Some(Anomaly {
                id: Uuid::new_v4(),
                detected_at: Utc::now(),
                anomaly_type: AnomalyType::Deadlock,
                severity: Severity::Critical,
                affected_agents: vec![agent_id],
                description: format!(
                    "Potential deadlock: {} consecutive samples with zero throughput and queue depth ≥ {} (max: {})",
                    self.config.deadlock_window, self.config.deadlock_queue_threshold, max_queue
                ),
                evidence: tail
                    .iter()
                    .map(|s| Evidence {
                        metric: "queue_depth".into(),
                        expected: 0.0,
                        observed: s.queue_depth as f64,
                        timestamp: s.timestamp,
                    })
                    .collect(),
            })
        } else {
            None
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(agent_id: AgentId, latency: f64, throughput: f64, progress: f64, queue: u64) -> MetricSample {
        MetricSample {
            agent_id,
            timestamp: Utc::now(),
            latency_ms: latency,
            throughput,
            error_rate: 0.0,
            progress,
            queue_depth: queue,
        }
    }

    #[test]
    fn test_spike_detection() {
        let agent = AgentId::new();
        let mut det = AnomalyDetector::new(DetectorConfig::default(), 100);

        // Normal samples with slight variation
        for i in 0..10 {
            det.ingest(sample(agent, 5.0 + (i as f64 * 0.1), 100.0, 1.0, 0));
        }

        // Spike
        let anomalies = det.ingest(sample(agent, 50.0, 100.0, 1.0, 0));
        assert!(!anomalies.is_empty());
        assert_eq!(anomalies[0].anomaly_type, AnomalyType::Spike);
    }

    #[test]
    fn test_no_spike_normal_variation() {
        let agent = AgentId::new();
        let mut det = AnomalyDetector::new(DetectorConfig::default(), 100);

        for i in 0..10 {
            det.ingest(sample(agent, 5.0 + (i as f64 % 3.0), 100.0, 1.0, 0));
        }

        // Slight variation — no spike
        let anomalies = det.ingest(sample(agent, 7.0, 100.0, 1.0, 0));
        let spikes: Vec<_> = anomalies.iter().filter(|a| a.anomaly_type == AnomalyType::Spike).collect();
        assert!(spikes.is_empty());
    }

    #[test]
    fn test_drift_detection() {
        let agent = AgentId::new();
        let mut det = AnomalyDetector::new(
            DetectorConfig {
                drift_min_samples: 5,
                drift_slope_threshold: -0.1,
                ..Default::default()
            },
            100,
        );

        // Throughput degrades linearly
        let mut last_anomalies = vec![];
        for i in 0..10 {
            last_anomalies = det.ingest(sample(agent, 5.0, 100.0 - i as f64 * 5.0, 1.0, 0));
        }

        let drifts: Vec<_> = last_anomalies.iter().filter(|a| a.anomaly_type == AnomalyType::Drift).collect();
        assert!(!drifts.is_empty());
    }

    #[test]
    fn test_livelock_detection() {
        let agent = AgentId::new();
        let mut det = AnomalyDetector::new(
            DetectorConfig {
                livelock_window: 3,
                livelock_progress_threshold: 0.01,
                ..Default::default()
            },
            100,
        );

        // Agent busy (throughput > 0) but no progress
        let mut last = vec![];
        for _ in 0..5 {
            last = det.ingest(sample(agent, 5.0, 50.0, 0.001, 0));
        }

        let livelocks: Vec<_> = last.iter().filter(|a| a.anomaly_type == AnomalyType::Livelock).collect();
        assert!(!livelocks.is_empty());
    }

    #[test]
    fn test_deadlock_detection() {
        let agent = AgentId::new();
        let mut det = AnomalyDetector::new(
            DetectorConfig {
                deadlock_window: 3,
                deadlock_queue_threshold: 5,
                ..Default::default()
            },
            100,
        );

        // Zero throughput with growing queue
        let mut last = vec![];
        for _ in 0..5 {
            last = det.ingest(sample(agent, 5.0, 0.0, 0.0, 20));
        }

        let deadlocks: Vec<_> = last.iter().filter(|a| a.anomaly_type == AnomalyType::Deadlock).collect();
        assert!(!deadlocks.is_empty());
        assert_eq!(deadlocks[0].severity, Severity::Critical);
    }

    #[test]
    fn test_no_deadlock_with_throughput() {
        let agent = AgentId::new();
        let mut det = AnomalyDetector::new(
            DetectorConfig {
                deadlock_window: 3,
                deadlock_queue_threshold: 5,
                ..Default::default()
            },
            100,
        );

        // Queue is high but throughput is non-zero
        let mut last = vec![];
        for _ in 0..5 {
            last = det.ingest(sample(agent, 5.0, 10.0, 1.0, 20));
        }

        let deadlocks: Vec<_> = last.iter().filter(|a| a.anomaly_type == AnomalyType::Deadlock).collect();
        assert!(deadlocks.is_empty());
    }

    #[test]
    fn test_anomalies_by_type() {
        let agent = AgentId::new();
        let mut det = AnomalyDetector::new(
            DetectorConfig {
                deadlock_window: 2,
                deadlock_queue_threshold: 5,
                ..Default::default()
            },
            100,
        );

        for _ in 0..5 {
            det.ingest(sample(agent, 5.0, 0.0, 0.0, 20));
        }

        assert!(!det.anomalies_by_type(AnomalyType::Deadlock).is_empty());
        assert!(det.anomalies_by_type(AnomalyType::Livelock).is_empty());
    }

    #[test]
    fn test_anomalies_above_severity() {
        let agent = AgentId::new();
        let mut det = AnomalyDetector::new(
            DetectorConfig {
                deadlock_window: 2,
                deadlock_queue_threshold: 5,
                ..Default::default()
            },
            100,
        );

        for _ in 0..5 {
            det.ingest(sample(agent, 5.0, 0.0, 0.0, 20));
        }

        let critical = det.anomalies_above_severity(Severity::Critical);
        assert!(!critical.is_empty());
    }

    #[test]
    fn test_clear_anomalies() {
        let agent = AgentId::new();
        let mut det = AnomalyDetector::new(
            DetectorConfig {
                deadlock_window: 2,
                deadlock_queue_threshold: 5,
                ..Default::default()
            },
            100,
        );

        for _ in 0..5 {
            det.ingest(sample(agent, 5.0, 0.0, 0.0, 20));
        }

        assert!(!det.anomalies().is_empty());
        det.clear();
        assert!(det.anomalies().is_empty());
    }

    #[test]
    fn test_window_eviction() {
        let agent = AgentId::new();
        let mut det = AnomalyDetector::new(DetectorConfig::default(), 5);

        for _ in 0..10 {
            det.ingest(sample(agent, 5.0, 100.0, 1.0, 0));
        }

        assert_eq!(det.windows.get(&agent).unwrap().len(), 5);
    }

    #[test]
    fn test_clear_agent() {
        let agent = AgentId::new();
        let mut det = AnomalyDetector::new(DetectorConfig::default(), 100);

        det.ingest(sample(agent, 5.0, 100.0, 1.0, 0));
        assert!(det.windows.contains_key(&agent));

        det.clear_agent(agent);
        assert!(!det.windows.contains_key(&agent));
    }
}
