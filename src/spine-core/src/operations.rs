//! Aggregate diagnostics for native admission and outbound navigation.
use crate::{connections::ConnectionSnapshot, outbound::FetchSnapshot};

#[derive(Debug, serde::Serialize)]
pub struct Readiness {
    pub ready: bool,
    pub accepting_connections: bool,
    pub accepting_sessions: bool,
    pub reasons: Vec<&'static str>,
    pub active_connections: usize,
    pub max_connections: usize,
    pub sessions: usize,
    pub max_sessions: usize,
}

pub fn readiness(
    draining: bool,
    sessions: usize,
    max_sessions: usize,
    connections: ConnectionSnapshot,
) -> Readiness {
    let mut reasons = Vec::new();
    if draining {
        reasons.push("draining");
    }
    if sessions >= max_sessions {
        reasons.push("session_capacity");
    }
    if connections.active >= connections.limit {
        reasons.push("connection_capacity");
    }
    Readiness {
        ready: reasons.is_empty(),
        accepting_connections: !draining && connections.active < connections.limit,
        accepting_sessions: !draining && sessions < max_sessions,
        reasons,
        active_connections: connections.active,
        max_connections: connections.limit,
        sessions,
        max_sessions,
    }
}

/// Fixed names and no user-controlled labels keep metrics cardinality bounded.
pub fn append_metrics(body: &mut String, c: ConnectionSnapshot, f: FetchSnapshot) {
    for (name, kind, value) in [
        ("native_connections_active", "gauge", c.active as u64),
        ("native_connections_limit", "gauge", c.limit as u64),
        (
            "native_connections_per_ip_limit",
            "gauge",
            c.per_ip_limit as u64,
        ),
        (
            "native_connections_rejected_global_total",
            "counter",
            c.rejected_global,
        ),
        (
            "native_connections_rejected_per_ip_total",
            "counter",
            c.rejected_per_ip,
        ),
        ("outbound_enabled", "gauge", u64::from(f.enabled)),
        ("outbound_requests_active", "gauge", f.active as u64),
        ("outbound_requests_limit", "gauge", f.limit as u64),
        ("outbound_calls_started_total", "counter", f.started),
        ("outbound_calls_succeeded_total", "counter", f.succeeded),
        ("outbound_calls_failed_total", "counter", f.failed),
        ("outbound_calls_cancelled_total", "counter", f.cancelled),
        ("outbound_calls_denied_total", "counter", f.denied),
        ("outbound_calls_busy_total", "counter", f.busy),
        ("outbound_calls_timed_out_total", "counter", f.timed_out),
        (
            "outbound_response_limit_failures_total",
            "counter",
            f.response_limit_failures,
        ),
        ("outbound_bytes_received_total", "counter", f.bytes_received),
    ] {
        body.push_str(&format!(
            "# TYPE spine_{name} {kind}\nspine_{name} {value}\n"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_tracks_capacity_recovery_and_draining() {
        let budget = crate::connections::ConnectionBudget::new(1, 1);
        assert!(readiness(false, 0, 1, budget.snapshot()).ready);
        let lease = budget.try_acquire("127.0.0.1".parse().unwrap()).unwrap();
        let saturated = readiness(false, 0, 1, budget.snapshot());
        assert!(!saturated.ready);
        assert!(saturated.accepting_sessions);
        assert!(!saturated.accepting_connections);
        assert_eq!(saturated.reasons, ["connection_capacity"]);
        drop(lease);
        assert!(readiness(false, 0, 1, budget.snapshot()).ready);
        assert_eq!(
            readiness(false, 1, 1, budget.snapshot()).reasons,
            ["session_capacity"]
        );
        let draining = readiness(true, 0, 1, budget.snapshot());
        assert_eq!(draining.reasons, ["draining"]);
        assert!(!draining.accepting_sessions && !draining.accepting_connections);
    }
}
