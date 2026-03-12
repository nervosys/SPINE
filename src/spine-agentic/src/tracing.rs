// =============================================================================
// TRACING: Distributed Tracing Across Mesh Hops
// =============================================================================
//
// Propagates trace context across agent mesh messages, enabling end-to-end
// visibility into multi-hop agent interactions. Builds on OpenTelemetry
// concepts (trace_id, span_id, parent_span_id) without requiring the full
// OTel runtime — operates at the agent protocol level.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::AgentId;

/// A globally unique trace identifier, scoping a causal chain of agent actions.
pub type TraceId = Uuid;
/// A unique identifier for a single span within a trace.
pub type SpanId = Uuid;

/// A span representing one unit of work in a distributed agent trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpan {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub parent_span_id: Option<SpanId>,
    pub agent_id: AgentId,
    pub operation: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub status: SpanStatus,
    pub attributes: HashMap<String, SpanValue>,
    pub events: Vec<SpanEvent>,
    /// Mesh hop count at the point this span was created.
    pub hop_count: u32,
}

impl AgentSpan {
    /// Start a new root span (no parent).
    pub fn root(agent_id: AgentId, operation: impl Into<String>) -> Self {
        Self {
            trace_id: Uuid::new_v4(),
            span_id: Uuid::new_v4(),
            parent_span_id: None,
            agent_id,
            operation: operation.into(),
            start_time: Utc::now(),
            end_time: None,
            status: SpanStatus::InProgress,
            attributes: HashMap::new(),
            events: Vec::new(),
            hop_count: 0,
        }
    }

    /// Start a child span from a parent context.
    pub fn child(
        ctx: &TraceContext,
        agent_id: AgentId,
        operation: impl Into<String>,
    ) -> Self {
        Self {
            trace_id: ctx.trace_id,
            span_id: Uuid::new_v4(),
            parent_span_id: Some(ctx.span_id),
            agent_id,
            operation: operation.into(),
            start_time: Utc::now(),
            end_time: None,
            status: SpanStatus::InProgress,
            attributes: HashMap::new(),
            events: Vec::new(),
            hop_count: ctx.hop_count + 1,
        }
    }

    /// Finish this span successfully.
    pub fn finish(&mut self) {
        self.end_time = Some(Utc::now());
        self.status = SpanStatus::Ok;
    }

    /// Finish this span with an error.
    pub fn finish_error(&mut self, message: impl Into<String>) {
        self.end_time = Some(Utc::now());
        self.status = SpanStatus::Error(message.into());
    }

    /// Add an attribute.
    pub fn set_attribute(&mut self, key: impl Into<String>, value: SpanValue) {
        self.attributes.insert(key.into(), value);
    }

    /// Add a timestamped event.
    pub fn add_event(&mut self, name: impl Into<String>, attrs: HashMap<String, SpanValue>) {
        self.events.push(SpanEvent {
            name: name.into(),
            timestamp: Utc::now(),
            attributes: attrs,
        });
    }

    /// Duration of this span (None if still in progress).
    pub fn duration_ms(&self) -> Option<i64> {
        self.end_time
            .map(|end| (end - self.start_time).num_milliseconds())
    }

    /// Extract a propagation context from this span.
    pub fn context(&self) -> TraceContext {
        TraceContext {
            trace_id: self.trace_id,
            span_id: self.span_id,
            hop_count: self.hop_count,
            baggage: HashMap::new(),
        }
    }
}

/// Status of a span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpanStatus {
    InProgress,
    Ok,
    Error(String),
}

/// Typed span attribute values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpanValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// A timestamped event within a span.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp: DateTime<Utc>,
    pub attributes: HashMap<String, SpanValue>,
}

/// Propagation context carried across mesh hops.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub hop_count: u32,
    /// Key-value baggage propagated across all spans in a trace.
    pub baggage: HashMap<String, String>,
}

impl TraceContext {
    /// Create a new root context.
    pub fn new() -> Self {
        Self {
            trace_id: Uuid::new_v4(),
            span_id: Uuid::new_v4(),
            hop_count: 0,
            baggage: HashMap::new(),
        }
    }

    /// Serialize to a header-friendly string for wire propagation.
    pub fn to_header(&self) -> String {
        format!("{}-{}-{}", self.trace_id, self.span_id, self.hop_count)
    }

    /// Parse from header string.
    pub fn from_header(header: &str) -> Option<Self> {
        let parts: Vec<&str> = header.split('-').collect();
        // UUID has 5 dash-separated groups, so trace_id is parts[0..5], span_id is [5..10], hop is [10]
        if parts.len() < 11 {
            return None;
        }
        let trace_str = parts[..5].join("-");
        let span_str = parts[5..10].join("-");
        let hop: u32 = parts[10].parse().ok()?;
        Some(Self {
            trace_id: Uuid::parse_str(&trace_str).ok()?,
            span_id: Uuid::parse_str(&span_str).ok()?,
            hop_count: hop,
            baggage: HashMap::new(),
        })
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Collects spans from multiple agents into a unified trace view.
pub struct TraceCollector {
    /// All spans indexed by trace_id.
    traces: DashMap<TraceId, Vec<AgentSpan>>,
    /// Maximum traces to retain.
    max_traces: usize,
}

impl TraceCollector {
    pub fn new(max_traces: usize) -> Self {
        Self {
            traces: DashMap::new(),
            max_traces,
        }
    }

    /// Record a completed span.
    pub fn record(&self, span: AgentSpan) {
        // Enforce capacity by evicting oldest if needed
        if !self.traces.contains_key(&span.trace_id) && self.traces.len() >= self.max_traces {
            // Remove the entry with the earliest first-span start time
            if let Some(oldest) = self
                .traces
                .iter()
                .min_by_key(|e| {
                    e.value()
                        .first()
                        .map(|s| s.start_time)
                        .unwrap_or_else(Utc::now)
                })
                .map(|e| *e.key())
            {
                self.traces.remove(&oldest);
            }
        }
        self.traces
            .entry(span.trace_id)
            .or_default()
            .push(span);
    }

    /// Get all spans for a trace, ordered by start time.
    pub fn get_trace(&self, trace_id: TraceId) -> Vec<AgentSpan> {
        self.traces
            .get(&trace_id)
            .map(|spans| {
                let mut sorted = spans.clone();
                sorted.sort_by_key(|s| s.start_time);
                sorted
            })
            .unwrap_or_default()
    }

    /// Get the root span of a trace.
    pub fn root_span(&self, trace_id: TraceId) -> Option<AgentSpan> {
        self.traces.get(&trace_id).and_then(|spans| {
            spans
                .iter()
                .find(|s| s.parent_span_id.is_none())
                .cloned()
        })
    }

    /// Total duration of a trace (root start → last span end).
    pub fn trace_duration_ms(&self, trace_id: TraceId) -> Option<i64> {
        let spans = self.traces.get(&trace_id)?;
        let start = spans.iter().map(|s| s.start_time).min()?;
        let end = spans.iter().filter_map(|s| s.end_time).max()?;
        Some((end - start).num_milliseconds())
    }

    /// Max hop count in a trace.
    pub fn max_hops(&self, trace_id: TraceId) -> u32 {
        self.traces
            .get(&trace_id)
            .map(|spans| spans.iter().map(|s| s.hop_count).max().unwrap_or(0))
            .unwrap_or(0)
    }

    /// Count of active traces.
    pub fn trace_count(&self) -> usize {
        self.traces.len()
    }

    /// List all trace IDs with their span counts.
    pub fn list_traces(&self) -> Vec<(TraceId, usize)> {
        self.traces
            .iter()
            .map(|e| (*e.key(), e.value().len()))
            .collect()
    }

    /// Spans involving a specific agent.
    pub fn agent_spans(&self, agent_id: AgentId) -> Vec<AgentSpan> {
        let mut result = Vec::new();
        for entry in self.traces.iter() {
            for span in entry.value() {
                if span.agent_id == agent_id {
                    result.push(span.clone());
                }
            }
        }
        result.sort_by_key(|s| s.start_time);
        result
    }

    /// Find slow spans (duration exceeding threshold).
    pub fn slow_spans(&self, threshold_ms: i64) -> Vec<AgentSpan> {
        let mut result = Vec::new();
        for entry in self.traces.iter() {
            for span in entry.value() {
                if let Some(dur) = span.duration_ms() {
                    if dur > threshold_ms {
                        result.push(span.clone());
                    }
                }
            }
        }
        result
    }

    /// Find error spans.
    pub fn error_spans(&self) -> Vec<AgentSpan> {
        let mut result = Vec::new();
        for entry in self.traces.iter() {
            for span in entry.value() {
                if matches!(span.status, SpanStatus::Error(_)) {
                    result.push(span.clone());
                }
            }
        }
        result
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_and_child_spans() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();

        let mut root = AgentSpan::root(agent_a, "search");
        root.set_attribute("query", SpanValue::String("test".into()));
        let ctx = root.context();
        root.finish();

        let mut child = AgentSpan::child(&ctx, agent_b, "sub-search");
        child.finish();

        assert_eq!(child.trace_id, root.trace_id);
        assert_eq!(child.parent_span_id, Some(root.span_id));
        assert_eq!(child.hop_count, 1);
    }

    #[test]
    fn test_span_duration() {
        let agent = AgentId::new();
        let mut span = AgentSpan::root(agent, "task");
        assert!(span.duration_ms().is_none());
        span.finish();
        assert!(span.duration_ms().unwrap() >= 0);
    }

    #[test]
    fn test_span_error() {
        let agent = AgentId::new();
        let mut span = AgentSpan::root(agent, "fail");
        span.finish_error("timeout");
        assert_eq!(span.status, SpanStatus::Error("timeout".into()));
    }

    #[test]
    fn test_span_events() {
        let agent = AgentId::new();
        let mut span = AgentSpan::root(agent, "work");
        span.add_event("checkpoint", HashMap::new());
        span.add_event("retry", HashMap::from([("attempt".into(), SpanValue::Int(2))]));
        assert_eq!(span.events.len(), 2);
        assert_eq!(span.events[1].name, "retry");
    }

    #[test]
    fn test_trace_context_header_roundtrip() {
        let ctx = TraceContext::new();
        let header = ctx.to_header();
        let parsed = TraceContext::from_header(&header).unwrap();
        assert_eq!(parsed.trace_id, ctx.trace_id);
        assert_eq!(parsed.span_id, ctx.span_id);
        assert_eq!(parsed.hop_count, ctx.hop_count);
    }

    #[test]
    fn test_collector_record_and_get() {
        let collector = TraceCollector::new(100);
        let agent = AgentId::new();

        let mut span = AgentSpan::root(agent, "op");
        span.finish();
        let trace_id = span.trace_id;
        collector.record(span);

        let spans = collector.get_trace(trace_id);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].operation, "op");
    }

    #[test]
    fn test_collector_root_span() {
        let collector = TraceCollector::new(100);
        let a = AgentId::new();
        let b = AgentId::new();

        let mut root = AgentSpan::root(a, "root_op");
        let ctx = root.context();
        root.finish();
        let tid = root.trace_id;
        collector.record(root);

        let mut child = AgentSpan::child(&ctx, b, "child_op");
        child.finish();
        collector.record(child);

        let root = collector.root_span(tid).unwrap();
        assert_eq!(root.operation, "root_op");
        assert!(root.parent_span_id.is_none());
    }

    #[test]
    fn test_collector_max_hops() {
        let collector = TraceCollector::new(100);
        let a = AgentId::new();
        let b = AgentId::new();
        let c = AgentId::new();

        let mut root = AgentSpan::root(a, "start");
        let ctx1 = root.context();
        root.finish();
        let tid = root.trace_id;
        collector.record(root);

        let mut hop1 = AgentSpan::child(&ctx1, b, "hop1");
        let ctx2 = hop1.context();
        hop1.finish();
        collector.record(hop1);

        let mut hop2 = AgentSpan::child(&ctx2, c, "hop2");
        hop2.finish();
        collector.record(hop2);

        assert_eq!(collector.max_hops(tid), 2);
    }

    #[test]
    fn test_collector_slow_spans() {
        let collector = TraceCollector::new(100);
        let agent = AgentId::new();

        let mut fast = AgentSpan::root(agent, "fast");
        fast.finish();
        collector.record(fast);

        // Simulate slow span
        let mut slow = AgentSpan::root(agent, "slow");
        slow.start_time = Utc::now() - chrono::Duration::seconds(5);
        slow.finish();
        collector.record(slow);

        let slow_spans = collector.slow_spans(1000);
        assert_eq!(slow_spans.len(), 1);
        assert_eq!(slow_spans[0].operation, "slow");
    }

    #[test]
    fn test_collector_error_spans() {
        let collector = TraceCollector::new(100);
        let agent = AgentId::new();

        let mut ok_span = AgentSpan::root(agent, "ok");
        ok_span.finish();
        collector.record(ok_span);

        let mut err_span = AgentSpan::root(agent, "err");
        err_span.finish_error("broken");
        collector.record(err_span);

        let errors = collector.error_spans();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].operation, "err");
    }

    #[test]
    fn test_collector_capacity_eviction() {
        let collector = TraceCollector::new(2);
        let agent = AgentId::new();

        for _ in 0..3 {
            let mut s = AgentSpan::root(agent, "op");
            s.finish();
            collector.record(s);
        }

        assert!(collector.trace_count() <= 2);
    }

    #[test]
    fn test_agent_spans() {
        let collector = TraceCollector::new(100);
        let a = AgentId::new();
        let b = AgentId::new();

        let mut s1 = AgentSpan::root(a, "a_op");
        s1.finish();
        collector.record(s1);

        let mut s2 = AgentSpan::root(b, "b_op");
        s2.finish();
        collector.record(s2);

        assert_eq!(collector.agent_spans(a).len(), 1);
        assert_eq!(collector.agent_spans(b).len(), 1);
    }
}
