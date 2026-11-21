# Observability & Debugging

SPINE provides observability tools for monitoring, debugging, and analyzing agent behavior.

**Research.** Distributed tracing, the replay debugger, the swarm visualizer and anomaly detection described here are in `research/spine-agentic` (`tracing.rs`, `replay.rs`, `visualizer.rs`, `anomaly.rs`), a separate workspace. The Grafana dashboard and Prometheus config in `deploy/` are separate from that code. The native `tool_call` command carries an optional `trace` field.

## Distributed Tracing

The `AgentSpan` system provides hierarchical trace context with:

- **Trace/span/parent IDs** for causality tracking
- **SpanEvent** with timestamps for fine-grained timing
- **TraceContext** wire propagation via header serialization
- **TraceCollector** with capacity-bounded storage

Query API: `root_span()`, `max_hops()`, `slow_spans(threshold)`, `error_spans()`, `agent_spans(id)`.

## Agent Replay Debugger

Record and replay agent decision-making:

- **ReplayLog** with SHA-256 integrity verification
- **7 entry kinds**: MessageReceived/Sent, Decision, StateTransition, TaskAction, ExternalEvent, Error
- **Breakpoints** and state verification callbacks
- **Divergence detection** between two replay logs

## Swarm Visualizer

Generate snapshots of swarm topology for visualization:

- **SwarmSnapshot**: nodes, edges, clusters, message flows, resource heatmap
- **SnapshotRecorder**: time-series extraction for load and message volume
- **NodeSnapshot**: state, load, capabilities per agent
- **EdgeSnapshot**: latency, bandwidth per connection

## Anomaly Detection

Automated detection of operational anomalies:

| Detector | Trigger |
|----------|---------|
| Spike | σ-based deviation from rolling mean |
| Drift | Linear regression slope on throughput |
| Livelock | Low progress + non-zero throughput |
| Deadlock | Zero throughput + high queue depth |

## Grafana Dashboard

A pre-built Grafana dashboard (`deploy/grafana/spine-dashboard.json`) with 12 panels covers all SPINE metrics. Prometheus scrape config is at `deploy/prometheus/prometheus.yml`.