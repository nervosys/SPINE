# Observability & Debugging

SPINE provides comprehensive observability tools for monitoring, debugging, and analyzing agent behavior.

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