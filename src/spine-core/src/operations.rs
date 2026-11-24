//! Aggregate diagnostics for native admission and outbound navigation, and the
//! pure pieces behind `/ready` and `/metrics`.
use crate::{connections::ConnectionSnapshot, outbound::FetchSnapshot};
use spine_mesh::naming_mesh::ResolverMetrics;
use std::fmt::Write as _;

#[derive(Debug, serde::Serialize)]
pub struct Readiness {
    pub ready: bool,
    pub accepting_connections: bool,
    pub accepting_sessions: bool,
    /// Why the node is not ready. Any entry here makes `/ready` fail.
    pub reasons: Vec<&'static str>,
    /// Conditions worth an operator's attention that do not make the node
    /// unready: a node with no peers may be a mesh's first node by design.
    pub warnings: Vec<&'static str>,
    pub active_connections: usize,
    pub max_connections: usize,
    pub sessions: usize,
    pub max_sessions: usize,
}

/// State of the dependencies a node needs besides its own capacity.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dependencies {
    /// The last attempt to persist sessions failed (it clears on the next
    /// success).
    pub persistence_failing: bool,
    /// The resource store's accounting could not be read.
    pub storage_unavailable: bool,
    /// The node has joined (or tried to join) the namespace mesh.
    pub namespace_enabled: bool,
    /// Addressable keyspace peers.
    pub namespace_peers: usize,
    /// Seeds or DNS seeds are configured.
    pub seeds_configured: bool,
}

/// Capacity-only readiness: no dependency is considered.
pub fn readiness(
    draining: bool,
    sessions: usize,
    max_sessions: usize,
    connections: ConnectionSnapshot,
) -> Readiness {
    readiness_with(
        draining,
        sessions,
        max_sessions,
        connections,
        Dependencies::default(),
    )
}

/// Readiness including dependency state.
///
/// A failing persistence backend or an unreadable resource store makes the node
/// unready. A namespace node with no peers does not: a first node of a mesh and
/// a deliberately solitary node have none by design, so that is a warning
/// (`namespace_seeds_unreachable` when seeds are configured and none answered,
/// else `namespace_no_peers`).
pub fn readiness_with(
    draining: bool,
    sessions: usize,
    max_sessions: usize,
    connections: ConnectionSnapshot,
    deps: Dependencies,
) -> Readiness {
    let mut reasons = Vec::new();
    let mut warnings = Vec::new();
    if draining {
        reasons.push("draining");
    }
    if sessions >= max_sessions {
        reasons.push("session_capacity");
    }
    if connections.active >= connections.limit {
        reasons.push("connection_capacity");
    }
    if deps.persistence_failing {
        reasons.push("persistence_failing");
    }
    if deps.storage_unavailable {
        reasons.push("storage_unavailable");
    }
    if deps.namespace_enabled && deps.namespace_peers == 0 {
        warnings.push(if deps.seeds_configured {
            "namespace_seeds_unreachable"
        } else {
            "namespace_no_peers"
        });
    }
    Readiness {
        ready: reasons.is_empty(),
        accepting_connections: !draining && connections.active < connections.limit,
        accepting_sessions: !draining && sessions < max_sessions,
        reasons,
        warnings,
        active_connections: connections.active,
        max_connections: connections.limit,
        sessions,
        max_sessions,
    }
}

/// Name cache counters of the local resolver.
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheCounts {
    pub hits: u64,
    pub stale_hits: u64,
    pub negative_hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

/// What the mesh resolver reports; present only when the namespace is enabled.
#[derive(Debug, Clone, Copy)]
pub struct MeshSnapshot {
    pub peers: usize,
    pub pending_lookups: usize,
    pub pending_calls: usize,
    pub metrics: ResolverMetrics,
}

/// Everything `/metrics` reports beyond the process-wide registry.
#[derive(Debug)]
pub struct MetricsSnapshot {
    pub sessions_limit: usize,
    pub draining: bool,
    pub connections: ConnectionSnapshot,
    pub outbound: FetchSnapshot,
    pub execute_permits_in_use: usize,
    pub execute_permits_limit: usize,
    pub persistence_failing: bool,
    /// `None` when the store's accounting could not be read.
    pub resource_bytes: Option<u64>,
    pub resource_limit_bytes: u64,
    pub records_held: usize,
    pub cache: CacheCounts,
    pub mesh: Option<MeshSnapshot>,
}

/// A label value in the text format: backslash, double quote and newline
/// escaped.
pub fn escape_label_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

/// A HELP text: backslash and newline escaped.
fn escape_help(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\n', "\\n")
}

fn header(out: &mut String, name: &str, help: &str, kind: &str) {
    let _ = writeln!(out, "# HELP spine_{name} {}", escape_help(help));
    let _ = writeln!(out, "# TYPE spine_{name} {kind}");
}

fn gauge(out: &mut String, name: &str, help: &str, value: u64) {
    header(out, name, help, "gauge");
    let _ = writeln!(out, "spine_{name} {value}");
}

fn counter(out: &mut String, name: &str, help: &str, value: u64) {
    header(out, name, help, "counter");
    let _ = writeln!(out, "spine_{name} {value}");
}

/// One counter family with a single fixed label. Label values are static
/// strings chosen here, never names, keys, peers, sessions or addresses.
fn counter_family(out: &mut String, name: &str, help: &str, label: &str, samples: &[(&str, u64)]) {
    header(out, name, help, "counter");
    for (value, count) in samples {
        let _ = writeln!(
            out,
            "spine_{name}{{{label}=\"{}\"}} {count}",
            escape_label_value(value)
        );
    }
}

/// Render the daemon's own metrics in the Prometheus text format (0.0.4), to be
/// appended to the process-wide registry's output. Every family has HELP and
/// TYPE, counters end in `_total`, and labels are drawn from fixed sets.
pub fn render_metrics(m: &MetricsSnapshot) -> String {
    let mut o = String::new();
    let out = &mut o;
    let c = m.connections;
    let f = &m.outbound;
    gauge(
        out,
        "sessions_limit",
        "Maximum retained sessions.",
        m.sessions_limit as u64,
    );
    gauge(
        out,
        "draining",
        "1 while the node is draining for shutdown.",
        u64::from(m.draining),
    );
    gauge(
        out,
        "native_connections_active",
        "Open native connections, including unfinished handshakes.",
        c.active as u64,
    );
    gauge(
        out,
        "native_connections_limit",
        "Maximum open native connections.",
        c.limit as u64,
    );
    gauge(
        out,
        "native_connections_per_ip_limit",
        "Maximum open native connections per address.",
        c.per_ip_limit as u64,
    );
    counter(
        out,
        "native_connections_rejected_global_total",
        "Connections refused at the global limit.",
        c.rejected_global,
    );
    counter(
        out,
        "native_connections_rejected_per_ip_total",
        "Connections refused at the per-address limit.",
        c.rejected_per_ip,
    );
    gauge(
        out,
        "outbound_enabled",
        "1 when outbound navigation is enabled.",
        u64::from(f.enabled),
    );
    gauge(
        out,
        "outbound_requests_active",
        "Outbound requests in flight.",
        f.active as u64,
    );
    gauge(
        out,
        "outbound_requests_limit",
        "Maximum concurrent outbound requests.",
        f.limit as u64,
    );
    for (name, help, value) in [
        (
            "outbound_calls_started_total",
            "Outbound calls started.",
            f.started,
        ),
        (
            "outbound_calls_succeeded_total",
            "Outbound calls that succeeded.",
            f.succeeded,
        ),
        (
            "outbound_calls_failed_total",
            "Outbound calls that failed.",
            f.failed,
        ),
        (
            "outbound_calls_cancelled_total",
            "Outbound calls cancelled.",
            f.cancelled,
        ),
        (
            "outbound_calls_denied_total",
            "Outbound calls denied by policy.",
            f.denied,
        ),
        (
            "outbound_calls_busy_total",
            "Outbound calls refused for lack of capacity.",
            f.busy,
        ),
        (
            "outbound_calls_timed_out_total",
            "Outbound calls that timed out.",
            f.timed_out,
        ),
        (
            "outbound_response_limit_failures_total",
            "Outbound calls that exceeded the response size limit.",
            f.response_limit_failures,
        ),
        (
            "outbound_bytes_received_total",
            "Bytes received by outbound calls.",
            f.bytes_received,
        ),
    ] {
        counter(out, name, help, value);
    }
    gauge(
        out,
        "execute_permits_in_use",
        "execute_binary programs compiling or running.",
        m.execute_permits_in_use as u64,
    );
    gauge(
        out,
        "execute_permits_limit",
        "Maximum concurrent execute_binary programs.",
        m.execute_permits_limit as u64,
    );
    gauge(
        out,
        "persistence_failing",
        "1 while the last session persistence attempt failed.",
        u64::from(m.persistence_failing),
    );
    gauge(
        out,
        "resource_store_available",
        "1 when the resource store's accounting is readable.",
        u64::from(m.resource_bytes.is_some()),
    );
    gauge(
        out,
        "resource_store_bytes",
        "Accounted bytes held by the resource store.",
        m.resource_bytes.unwrap_or(0),
    );
    gauge(
        out,
        "resource_store_limit_bytes",
        "Total byte limit of the resource store.",
        m.resource_limit_bytes,
    );
    gauge(
        out,
        "namespace_records_held",
        "Name records held locally.",
        m.records_held as u64,
    );
    counter_family(
        out,
        "namespace_cache_lookups_total",
        "Local name cache lookups by result.",
        "result",
        &[
            ("hit", m.cache.hits),
            ("stale_hit", m.cache.stale_hits),
            ("negative_hit", m.cache.negative_hits),
            ("miss", m.cache.misses),
        ],
    );
    counter(
        out,
        "namespace_cache_evictions_total",
        "Local name cache evictions.",
        m.cache.evictions,
    );
    gauge(
        out,
        "namespace_enabled",
        "1 when the node has joined the namespace mesh.",
        u64::from(m.mesh.is_some()),
    );
    if let Some(mesh) = &m.mesh {
        let r = &mesh.metrics;
        gauge(
            out,
            "namespace_peers",
            "Addressable keyspace peers known.",
            mesh.peers as u64,
        );
        gauge(
            out,
            "namespace_pending_lookups",
            "Lookups awaiting an answer.",
            mesh.pending_lookups as u64,
        );
        gauge(
            out,
            "mesh_pending_calls",
            "Application calls awaiting an answer.",
            mesh.pending_calls as u64,
        );
        for (name, help, value) in [
            (
                "namespace_lookups_started_total",
                "Name lookups started.",
                r.lookups_started,
            ),
            (
                "namespace_lookups_resolved_total",
                "Name lookups resolved.",
                r.lookups_resolved,
            ),
            (
                "namespace_lookups_timed_out_total",
                "Name lookups that timed out.",
                r.lookups_timed_out,
            ),
            (
                "namespace_requests_answered_total",
                "Lookup requests answered for peers.",
                r.requests_answered,
            ),
            (
                "namespace_announcements_accepted_total",
                "Announcements accepted.",
                r.announcements_accepted,
            ),
            (
                "namespace_announcements_rejected_total",
                "Announcements rejected as invalid.",
                r.announcements_rejected,
            ),
            (
                "namespace_announcements_declined_total",
                "Announcements declined for want of room or proximity.",
                r.announcements_declined,
            ),
            (
                "namespace_unroutable_peers_total",
                "Peers learned that could not be routed to.",
                r.unroutable_peers,
            ),
            (
                "namespace_greetings_answered_total",
                "Bootstrap hellos answered.",
                r.greetings_answered,
            ),
            (
                "namespace_greetings_rejected_total",
                "Hellos or acks with a bad signature.",
                r.greetings_rejected,
            ),
            (
                "namespace_replicas_sent_total",
                "Record replicas sent to closer nodes.",
                r.replicas_sent,
            ),
            (
                "namespace_replicas_failed_total",
                "Record replicas that could not be delivered.",
                r.replicas_failed,
            ),
            (
                "namespace_records_expired_total",
                "Records dropped because they expired.",
                r.records_expired,
            ),
            (
                "namespace_envelopes_unauthenticated_total",
                "Envelopes dropped for an unbindable sender.",
                r.envelopes_unauthenticated,
            ),
            (
                "namespace_agent_id_mismatches_total",
                "Agent id and key pairs refused as not matching.",
                r.agent_id_mismatches,
            ),
            (
                "namespace_equivocations_total",
                "Equivocation proofs recorded.",
                r.equivocations_found,
            ),
        ] {
            counter(out, name, help, value);
        }
        counter_family(
            out,
            "namespace_records_refused_total",
            "Records refused for succession reasons.",
            "reason",
            &[
                ("need_log", r.records_need_log),
                ("succession", r.records_refused_by_succession),
            ],
        );
        counter_family(
            out,
            "namespace_succession_logs_total",
            "Succession logs received by outcome.",
            "outcome",
            &[
                ("adopted", r.succession_logs_adopted),
                ("oversize", r.succession_logs_oversize),
                ("rejected", r.succession_logs_rejected),
            ],
        );
        counter_family(
            out,
            "namespace_log_fetches_total",
            "Requests for missing succession logs by outcome.",
            "outcome",
            &[
                ("sent", r.log_fetches_sent),
                ("suppressed", r.log_fetches_suppressed),
                ("adopted", r.logs_fetched),
            ],
        );
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conns(active: usize, limit: usize) -> ConnectionSnapshot {
        ConnectionSnapshot {
            active,
            limit,
            per_ip_limit: 2,
            rejected_global: 3,
            rejected_per_ip: 4,
        }
    }

    fn fetch() -> FetchSnapshot {
        FetchSnapshot {
            enabled: true,
            active: 1,
            limit: 16,
            started: 10,
            succeeded: 6,
            failed: 4,
            cancelled: 1,
            denied: 2,
            busy: 0,
            timed_out: 1,
            response_limit_failures: 1,
            bytes_received: 99,
        }
    }

    fn snapshot(mesh: Option<MeshSnapshot>) -> MetricsSnapshot {
        MetricsSnapshot {
            sessions_limit: 8,
            draining: false,
            connections: conns(1, 4),
            outbound: fetch(),
            execute_permits_in_use: 1,
            execute_permits_limit: 4,
            persistence_failing: false,
            resource_bytes: Some(1024),
            resource_limit_bytes: 4096,
            records_held: 7,
            cache: CacheCounts {
                hits: 5,
                stale_hits: 1,
                negative_hits: 2,
                misses: 3,
                evictions: 0,
            },
            mesh,
        }
    }

    fn mesh() -> MeshSnapshot {
        let metrics = ResolverMetrics {
            lookups_started: 11,
            records_need_log: 2,
            records_refused_by_succession: 3,
            logs_fetched: 1,
            ..Default::default()
        };
        MeshSnapshot {
            peers: 2,
            pending_lookups: 1,
            pending_calls: 0,
            metrics,
        }
    }

    fn valid_name(name: &str) -> bool {
        let mut chars = name.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == ':')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
    }

    /// Checks the whole text: names valid, one HELP and TYPE per family before
    /// its samples, counters end in `_total`, values numeric, labels quoted.
    fn check_exposition(text: &str) {
        let mut helped = std::collections::HashSet::new();
        let mut typed = std::collections::HashMap::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("# HELP ") {
                let name = rest.split(' ').next().unwrap();
                assert!(valid_name(name), "bad name {name}");
                assert!(helped.insert(name.to_string()), "duplicate HELP {name}");
            } else if let Some(rest) = line.strip_prefix("# TYPE ") {
                let mut parts = rest.split(' ');
                let (name, kind) = (parts.next().unwrap(), parts.next().unwrap());
                assert!(valid_name(name), "bad name {name}");
                assert!(["gauge", "counter"].contains(&kind));
                assert!(helped.contains(name), "TYPE before HELP for {name}");
                assert_eq!(kind == "counter", name.ends_with("_total"), "{name}");
                assert!(typed.insert(name.to_string(), kind).is_none());
            } else {
                let (series, value) = line.rsplit_once(' ').expect("sample has a value");
                assert!(value.parse::<f64>().is_ok(), "non-numeric value: {line}");
                let name = series.split('{').next().unwrap();
                assert!(valid_name(name), "bad name {name}");
                assert!(typed.contains_key(name), "sample without TYPE: {line}");
                if let Some(labels) = series.strip_prefix(name).filter(|l| !l.is_empty()) {
                    assert!(labels.starts_with('{') && labels.ends_with("\"}"));
                }
            }
        }
        assert_eq!(helped.len(), typed.len());
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn metrics_are_valid_exposition_with_and_without_a_mesh() {
        check_exposition(&render_metrics(&snapshot(None)));
        check_exposition(&render_metrics(&snapshot(Some(mesh()))));
    }

    #[test]
    fn metrics_output_is_exact_for_known_values() {
        let text = render_metrics(&snapshot(Some(mesh())));
        for line in [
            "# HELP spine_native_connections_active Open native connections, including unfinished handshakes.\n# TYPE spine_native_connections_active gauge\nspine_native_connections_active 1\n",
            "# TYPE spine_native_connections_rejected_global_total counter\nspine_native_connections_rejected_global_total 3\n",
            "spine_outbound_calls_started_total 10\n",
            "spine_execute_permits_in_use 1\n",
            "spine_resource_store_available 1\n",
            "spine_resource_store_bytes 1024\n",
            "spine_namespace_records_held 7\n",
            "spine_namespace_cache_lookups_total{result=\"negative_hit\"} 2\n",
            "spine_namespace_enabled 1\n",
            "spine_namespace_peers 2\n",
            "spine_namespace_lookups_started_total 11\n",
            "spine_namespace_records_refused_total{reason=\"need_log\"} 2\n",
            "spine_namespace_records_refused_total{reason=\"succession\"} 3\n",
            "spine_namespace_log_fetches_total{outcome=\"adopted\"} 1\n",
        ] {
            assert!(text.contains(line), "missing {line:?} in\n{text}");
        }
        let solo = render_metrics(&snapshot(None));
        assert!(solo.contains("spine_namespace_enabled 0\n"));
        assert!(!solo.contains("spine_namespace_peers"));
        // The process-wide registry already exports this gauge.
        assert!(!text.contains("spine_sessions_active"));
    }

    #[test]
    fn an_unreadable_store_reports_unavailable_not_a_size() {
        let mut s = snapshot(None);
        s.resource_bytes = None;
        s.persistence_failing = true;
        let text = render_metrics(&s);
        assert!(text.contains("spine_resource_store_available 0\n"));
        assert!(text.contains("spine_persistence_failing 1\n"));
    }

    #[test]
    fn label_values_are_escaped() {
        assert_eq!(escape_label_value("a\\b\"c\nd"), "a\\\\b\\\"c\\nd");
        assert_eq!(escape_label_value("plain"), "plain");
        assert_eq!(escape_help("a\\b\nc"), "a\\\\b\\nc");
        let mut out = String::new();
        counter_family(&mut out, "x_total", "h", "k", &[("a\"b", 1)]);
        assert!(out.contains("spine_x_total{k=\"a\\\"b\"} 1\n"));
    }

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

    #[test]
    fn a_peerless_namespace_node_warns_but_stays_ready() {
        let first = readiness_with(
            false,
            0,
            8,
            conns(0, 4),
            Dependencies {
                namespace_enabled: true,
                ..Default::default()
            },
        );
        assert!(first.ready && first.reasons.is_empty());
        assert_eq!(first.warnings, ["namespace_no_peers"]);
        let seeded = readiness_with(
            false,
            0,
            8,
            conns(0, 4),
            Dependencies {
                namespace_enabled: true,
                seeds_configured: true,
                ..Default::default()
            },
        );
        assert!(seeded.ready);
        assert_eq!(seeded.warnings, ["namespace_seeds_unreachable"]);
        let joined = readiness_with(
            false,
            0,
            8,
            conns(0, 4),
            Dependencies {
                namespace_enabled: true,
                namespace_peers: 1,
                seeds_configured: true,
                ..Default::default()
            },
        );
        assert!(joined.ready && joined.warnings.is_empty());
        let disabled = readiness_with(false, 0, 8, conns(0, 4), Dependencies::default());
        assert!(disabled.ready && disabled.warnings.is_empty());
    }

    #[test]
    fn storage_failures_make_the_node_unready() {
        let r = readiness_with(
            false,
            0,
            8,
            conns(0, 4),
            Dependencies {
                persistence_failing: true,
                storage_unavailable: true,
                ..Default::default()
            },
        );
        assert!(!r.ready);
        assert_eq!(r.reasons, ["persistence_failing", "storage_unavailable"]);
        assert!(r.accepting_sessions && r.accepting_connections);
    }
}
