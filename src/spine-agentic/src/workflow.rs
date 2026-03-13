// =============================================================================
// SPINE Workflow Orchestration Engine
// =============================================================================
//
// DAG-based task pipelines for composing multi-step agent workflows.
// Workflows define directed acyclic graphs of steps where each step
// can depend on the outputs of previous steps. The engine handles
// scheduling, dependency resolution, retry, timeout, and result
// propagation.
//
// =============================================================================

use chrono::{DateTime, Utc};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::toposort;
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

use crate::AgentId;

/// Unique identifier for a workflow.
pub type WorkflowId = Uuid;

/// Unique identifier for a step within a workflow.
pub type StepId = Uuid;

// =============================================================================
// STEP DEFINITIONS
// =============================================================================

/// The kind of operation a step performs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepKind {
    /// Fetch a URL and produce its HTML content.
    Fetch { url: String },
    /// Extract structured data using a named schema.
    Extract { schema_name: String },
    /// Transform data with a named transformer.
    Transform { transformer: String },
    /// Store results in the knowledge base.
    Store { namespace: String },
    /// Invoke an agent capability by name.
    AgentCall { capability: String },
    /// Run an HLS script.
    Script { source: String },
    /// Fan-out: duplicate input to N parallel branches.
    FanOut { branches: usize },
    /// Fan-in: merge results from parallel branches.
    FanIn,
    /// Conditional: choose a branch based on a predicate key in the input.
    Conditional { predicate_key: String },
    /// Custom step with opaque payload.
    Custom { kind: String, config: serde_json::Value },
}

/// Status of a workflow step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StepStatus {
    Pending,
    Ready,
    Running,
    Completed,
    Failed,
    Skipped,
    Cancelled,
}

/// A single step in a workflow DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: StepId,
    pub name: String,
    pub kind: StepKind,
    pub status: StepStatus,
    /// Agent assigned to execute this step (None = auto-assign).
    pub assigned_agent: Option<AgentId>,
    /// Required capabilities for the executing agent.
    pub required_capabilities: Vec<String>,
    /// Maximum retries on failure.
    pub max_retries: u32,
    pub attempt: u32,
    /// Timeout for this step.
    pub timeout_secs: Option<u64>,
    /// Input data (populated from upstream outputs or initial input).
    pub input: Option<serde_json::Value>,
    /// Output data (populated after completion).
    pub output: Option<serde_json::Value>,
    /// Error message on failure.
    pub error: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl WorkflowStep {
    pub fn new(name: impl Into<String>, kind: StepKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            kind,
            status: StepStatus::Pending,
            assigned_agent: None,
            required_capabilities: Vec::new(),
            max_retries: 2,
            attempt: 0,
            timeout_secs: None,
            input: None,
            output: None,
            error: None,
            started_at: None,
            completed_at: None,
        }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    pub fn with_agent(mut self, agent: AgentId) -> Self {
        self.assigned_agent = Some(agent);
        self
    }

    pub fn with_capabilities(mut self, caps: Vec<String>) -> Self {
        self.required_capabilities = caps;
        self
    }

    pub fn with_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }
}

// =============================================================================
// WORKFLOW DEFINITION
// =============================================================================

/// Status of the overall workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkflowStatus {
    /// Workflow has been defined but not started.
    Draft,
    /// Workflow is actively executing.
    Running,
    /// All steps completed successfully.
    Completed,
    /// One or more steps failed after retries.
    Failed,
    /// Workflow was cancelled by the user or system.
    Cancelled,
    /// Workflow is paused and can be resumed.
    Paused,
}

/// A complete workflow: a DAG of steps with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: WorkflowId,
    pub name: String,
    pub description: String,
    pub status: WorkflowStatus,
    pub steps: Vec<WorkflowStep>,
    /// Edges: (from_index, to_index) in the steps vec.
    pub edges: Vec<(usize, usize)>,
    /// Initial input data for source steps.
    pub initial_input: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    /// SHA-256 hash of the workflow definition for integrity.
    pub definition_hash: String,
}

impl Workflow {
    /// Compute the topological execution order of steps.
    /// Returns step indices in dependency-respecting order,
    /// or an error if the graph has a cycle.
    pub fn execution_order(&self) -> Result<Vec<usize>, WorkflowError> {
        let mut graph = DiGraph::<usize, ()>::new();
        let nodes: Vec<NodeIndex> = (0..self.steps.len())
            .map(|i| graph.add_node(i))
            .collect();
        for &(from, to) in &self.edges {
            if from >= self.steps.len() || to >= self.steps.len() {
                return Err(WorkflowError::InvalidEdge { from, to });
            }
            graph.add_edge(nodes[from], nodes[to], ());
        }
        toposort(&graph, None)
            .map(|sorted| sorted.into_iter().map(|n| graph[n]).collect())
            .map_err(|_| WorkflowError::CycleDetected)
    }

    /// Get indices of steps that are ready to execute:
    /// all their upstream dependencies are Completed.
    pub fn ready_steps(&self) -> Vec<usize> {
        let mut graph = DiGraph::<usize, ()>::new();
        let nodes: Vec<NodeIndex> = (0..self.steps.len())
            .map(|i| graph.add_node(i))
            .collect();
        for &(from, to) in &self.edges {
            if from < self.steps.len() && to < self.steps.len() {
                graph.add_edge(nodes[from], nodes[to], ());
            }
        }

        (0..self.steps.len())
            .filter(|&i| {
                self.steps[i].status == StepStatus::Pending
                    && graph
                        .neighbors_directed(nodes[i], Direction::Incoming)
                        .all(|pred| self.steps[graph[pred]].status == StepStatus::Completed)
            })
            .collect()
    }

    /// Returns true if the workflow is terminal (completed, failed, or cancelled).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            WorkflowStatus::Completed | WorkflowStatus::Failed | WorkflowStatus::Cancelled
        )
    }

    /// Count steps by status.
    pub fn step_counts(&self) -> HashMap<StepStatus, usize> {
        let mut counts = HashMap::new();
        for step in &self.steps {
            *counts.entry(step.status).or_insert(0) += 1;
        }
        counts
    }
}

// =============================================================================
// WORKFLOW BUILDER
// =============================================================================

/// Fluent builder for constructing workflows.
pub struct WorkflowBuilder {
    name: String,
    description: String,
    steps: Vec<WorkflowStep>,
    edges: Vec<(usize, usize)>,
    initial_input: Option<serde_json::Value>,
}

impl WorkflowBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            steps: Vec::new(),
            edges: Vec::new(),
            initial_input: None,
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Add a step and return its index.
    pub fn add_step(&mut self, step: WorkflowStep) -> usize {
        let idx = self.steps.len();
        self.steps.push(step);
        idx
    }

    /// Add a dependency edge: `from` must complete before `to` starts.
    pub fn add_edge(&mut self, from: usize, to: usize) -> &mut Self {
        self.edges.push((from, to));
        self
    }

    pub fn initial_input(mut self, input: serde_json::Value) -> Self {
        self.initial_input = Some(input);
        self
    }

    /// Build the workflow, computing its definition hash.
    pub fn build(self) -> Result<Workflow, WorkflowError> {
        if self.steps.is_empty() {
            return Err(WorkflowError::EmptyWorkflow);
        }

        // Validate edges reference valid indices
        for &(from, to) in &self.edges {
            if from >= self.steps.len() || to >= self.steps.len() {
                return Err(WorkflowError::InvalidEdge { from, to });
            }
        }

        // Verify no cycles
        let mut graph = DiGraph::<usize, ()>::new();
        let nodes: Vec<NodeIndex> = (0..self.steps.len())
            .map(|i| graph.add_node(i))
            .collect();
        for &(from, to) in &self.edges {
            graph.add_edge(nodes[from], nodes[to], ());
        }
        if toposort(&graph, None).is_err() {
            return Err(WorkflowError::CycleDetected);
        }

        // Compute definition hash
        let mut hasher = Sha256::new();
        hasher.update(self.name.as_bytes());
        for (i, step) in self.steps.iter().enumerate() {
            hasher.update(i.to_le_bytes());
            hasher.update(step.name.as_bytes());
        }
        for &(from, to) in &self.edges {
            hasher.update(from.to_le_bytes());
            hasher.update(to.to_le_bytes());
        }
        let hash = format!("{:x}", hasher.finalize());

        Ok(Workflow {
            id: Uuid::new_v4(),
            name: self.name,
            description: self.description,
            status: WorkflowStatus::Draft,
            steps: self.steps,
            edges: self.edges,
            initial_input: self.initial_input,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            definition_hash: hash,
        })
    }
}

// =============================================================================
// WORKFLOW ENGINE
// =============================================================================

/// Errors during workflow construction or execution.
#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("workflow has no steps")]
    EmptyWorkflow,
    #[error("cycle detected in workflow DAG")]
    CycleDetected,
    #[error("invalid edge: step {from} → {to} out of bounds")]
    InvalidEdge { from: usize, to: usize },
    #[error("workflow not found: {0}")]
    NotFound(WorkflowId),
    #[error("step not found: {0}")]
    StepNotFound(StepId),
    #[error("workflow {0} is not in a runnable state")]
    NotRunnable(WorkflowId),
    #[error("step {0} failed: {1}")]
    StepFailed(StepId, String),
}

/// An event emitted during workflow execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEvent {
    pub workflow_id: WorkflowId,
    pub step_id: Option<StepId>,
    pub kind: WorkflowEventKind,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkflowEventKind {
    WorkflowStarted,
    WorkflowCompleted,
    WorkflowFailed { reason: String },
    WorkflowCancelled,
    WorkflowPaused,
    WorkflowResumed,
    StepReady,
    StepStarted { agent: Option<AgentId> },
    StepCompleted,
    StepFailed { error: String, attempt: u32 },
    StepRetrying { attempt: u32 },
    StepSkipped,
}

/// The workflow engine: manages multiple workflows and drives execution.
pub struct WorkflowEngine {
    workflows: HashMap<WorkflowId, Workflow>,
    event_log: Vec<WorkflowEvent>,
}

impl WorkflowEngine {
    pub fn new() -> Self {
        Self {
            workflows: HashMap::new(),
            event_log: Vec::new(),
        }
    }

    /// Register a workflow for execution.
    pub fn register(&mut self, workflow: Workflow) -> WorkflowId {
        let id = workflow.id;
        self.workflows.insert(id, workflow);
        id
    }

    /// Get a workflow by ID.
    pub fn get(&self, id: &WorkflowId) -> Option<&Workflow> {
        self.workflows.get(id)
    }

    /// Get a mutable workflow by ID.
    pub fn get_mut(&mut self, id: &WorkflowId) -> Option<&mut Workflow> {
        self.workflows.get_mut(id)
    }

    /// List all workflow IDs and their statuses.
    pub fn list(&self) -> Vec<(WorkflowId, &str, WorkflowStatus)> {
        self.workflows
            .values()
            .map(|w| (w.id, w.name.as_str(), w.status))
            .collect()
    }

    /// Start a workflow: transition from Draft → Running and mark ready steps.
    pub fn start(&mut self, id: &WorkflowId) -> Result<Vec<usize>, WorkflowError> {
        let wf = self
            .workflows
            .get_mut(id)
            .ok_or(WorkflowError::NotFound(*id))?;

        if wf.status != WorkflowStatus::Draft && wf.status != WorkflowStatus::Paused {
            return Err(WorkflowError::NotRunnable(*id));
        }

        wf.status = WorkflowStatus::Running;
        wf.started_at = Some(Utc::now());

        self.event_log.push(WorkflowEvent {
            workflow_id: *id,
            step_id: None,
            kind: WorkflowEventKind::WorkflowStarted,
            timestamp: Utc::now(),
        });

        // Compute ready steps (no dependencies or all deps completed)
        let wf = self.workflows.get(id).unwrap();
        let ready = wf.ready_steps();

        // Mark them Ready
        let wf = self.workflows.get_mut(id).unwrap();
        for &idx in &ready {
            wf.steps[idx].status = StepStatus::Ready;
            // Propagate initial input to source steps
            if wf.steps[idx].input.is_none() {
                wf.steps[idx].input = wf.initial_input.clone();
            }
            self.event_log.push(WorkflowEvent {
                workflow_id: *id,
                step_id: Some(wf.steps[idx].id),
                kind: WorkflowEventKind::StepReady,
                timestamp: Utc::now(),
            });
        }

        Ok(ready)
    }

    /// Mark a step as started (Running).
    pub fn start_step(
        &mut self,
        workflow_id: &WorkflowId,
        step_idx: usize,
        agent: Option<AgentId>,
    ) -> Result<(), WorkflowError> {
        let wf = self
            .workflows
            .get_mut(workflow_id)
            .ok_or(WorkflowError::NotFound(*workflow_id))?;

        let step = wf
            .steps
            .get_mut(step_idx)
            .ok_or(WorkflowError::StepNotFound(Uuid::nil()))?;

        step.status = StepStatus::Running;
        step.started_at = Some(Utc::now());
        step.attempt += 1;
        step.assigned_agent = agent;
        let step_id = step.id;

        self.event_log.push(WorkflowEvent {
            workflow_id: *workflow_id,
            step_id: Some(step_id),
            kind: WorkflowEventKind::StepStarted { agent },
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Complete a step with output data. Returns newly-ready downstream step indices.
    pub fn complete_step(
        &mut self,
        workflow_id: &WorkflowId,
        step_idx: usize,
        output: serde_json::Value,
    ) -> Result<Vec<usize>, WorkflowError> {
        let wf = self
            .workflows
            .get_mut(workflow_id)
            .ok_or(WorkflowError::NotFound(*workflow_id))?;

        let step = wf
            .steps
            .get_mut(step_idx)
            .ok_or(WorkflowError::StepNotFound(Uuid::nil()))?;

        step.status = StepStatus::Completed;
        step.completed_at = Some(Utc::now());
        step.output = Some(output.clone());
        let step_id = step.id;

        self.event_log.push(WorkflowEvent {
            workflow_id: *workflow_id,
            step_id: Some(step_id),
            kind: WorkflowEventKind::StepCompleted,
            timestamp: Utc::now(),
        });

        // Propagate output to downstream steps as input
        let downstream: Vec<usize> = wf
            .edges
            .iter()
            .filter(|(from, _)| *from == step_idx)
            .map(|(_, to)| *to)
            .collect();

        for &ds_idx in &downstream {
            if let Some(ds) = wf.steps.get_mut(ds_idx) {
                ds.input = Some(output.clone());
            }
        }

        // Find newly ready steps
        let wf = self.workflows.get(workflow_id).unwrap();
        let newly_ready: Vec<usize> = wf
            .ready_steps()
            .into_iter()
            .collect();

        // Mark them Ready
        let wf = self.workflows.get_mut(workflow_id).unwrap();
        for &idx in &newly_ready {
            wf.steps[idx].status = StepStatus::Ready;
            self.event_log.push(WorkflowEvent {
                workflow_id: *workflow_id,
                step_id: Some(wf.steps[idx].id),
                kind: WorkflowEventKind::StepReady,
                timestamp: Utc::now(),
            });
        }

        // Check if workflow is complete
        let wf = self.workflows.get(workflow_id).unwrap();
        let all_done = wf
            .steps
            .iter()
            .all(|s| matches!(s.status, StepStatus::Completed | StepStatus::Skipped));

        if all_done {
            let wf = self.workflows.get_mut(workflow_id).unwrap();
            wf.status = WorkflowStatus::Completed;
            wf.completed_at = Some(Utc::now());
            self.event_log.push(WorkflowEvent {
                workflow_id: *workflow_id,
                step_id: None,
                kind: WorkflowEventKind::WorkflowCompleted,
                timestamp: Utc::now(),
            });
        }

        Ok(newly_ready)
    }

    /// Fail a step. Retries if attempts remain, otherwise marks as Failed.
    /// Returns true if the step will be retried, false if permanently failed.
    pub fn fail_step(
        &mut self,
        workflow_id: &WorkflowId,
        step_idx: usize,
        error: String,
    ) -> Result<bool, WorkflowError> {
        let wf = self
            .workflows
            .get_mut(workflow_id)
            .ok_or(WorkflowError::NotFound(*workflow_id))?;

        let step = wf
            .steps
            .get_mut(step_idx)
            .ok_or(WorkflowError::StepNotFound(Uuid::nil()))?;

        let step_id = step.id;

        if step.attempt < step.max_retries {
            // Retry
            step.status = StepStatus::Ready;
            step.error = Some(error.clone());
            self.event_log.push(WorkflowEvent {
                workflow_id: *workflow_id,
                step_id: Some(step_id),
                kind: WorkflowEventKind::StepRetrying {
                    attempt: step.attempt + 1,
                },
                timestamp: Utc::now(),
            });
            Ok(true)
        } else {
            // Permanent failure
            step.status = StepStatus::Failed;
            step.error = Some(error.clone());
            step.completed_at = Some(Utc::now());

            self.event_log.push(WorkflowEvent {
                workflow_id: *workflow_id,
                step_id: Some(step_id),
                kind: WorkflowEventKind::StepFailed {
                    error: error.clone(),
                    attempt: step.attempt,
                },
                timestamp: Utc::now(),
            });

            // Mark workflow as failed
            let wf = self.workflows.get_mut(workflow_id).unwrap();
            wf.status = WorkflowStatus::Failed;
            wf.completed_at = Some(Utc::now());
            self.event_log.push(WorkflowEvent {
                workflow_id: *workflow_id,
                step_id: None,
                kind: WorkflowEventKind::WorkflowFailed {
                    reason: format!("step failed: {error}"),
                },
                timestamp: Utc::now(),
            });

            Ok(false)
        }
    }

    /// Cancel a workflow.
    pub fn cancel(&mut self, id: &WorkflowId) -> Result<(), WorkflowError> {
        let wf = self
            .workflows
            .get_mut(id)
            .ok_or(WorkflowError::NotFound(*id))?;

        wf.status = WorkflowStatus::Cancelled;
        wf.completed_at = Some(Utc::now());
        for step in &mut wf.steps {
            if matches!(
                step.status,
                StepStatus::Pending | StepStatus::Ready | StepStatus::Running
            ) {
                step.status = StepStatus::Cancelled;
            }
        }

        self.event_log.push(WorkflowEvent {
            workflow_id: *id,
            step_id: None,
            kind: WorkflowEventKind::WorkflowCancelled,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Pause a running workflow.
    pub fn pause(&mut self, id: &WorkflowId) -> Result<(), WorkflowError> {
        let wf = self
            .workflows
            .get_mut(id)
            .ok_or(WorkflowError::NotFound(*id))?;

        if wf.status != WorkflowStatus::Running {
            return Err(WorkflowError::NotRunnable(*id));
        }

        wf.status = WorkflowStatus::Paused;

        self.event_log.push(WorkflowEvent {
            workflow_id: *id,
            step_id: None,
            kind: WorkflowEventKind::WorkflowPaused,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Get the event log for a workflow.
    pub fn events_for(&self, workflow_id: &WorkflowId) -> Vec<&WorkflowEvent> {
        self.event_log
            .iter()
            .filter(|e| e.workflow_id == *workflow_id)
            .collect()
    }

    /// Get all events.
    pub fn all_events(&self) -> &[WorkflowEvent] {
        &self.event_log
    }

    fn emit(&mut self, event: WorkflowEvent) {
        self.event_log.push(event);
    }
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// WORKFLOW TEMPLATES
// =============================================================================

/// Pre-built workflow templates for common agent patterns.
pub struct WorkflowTemplates;

impl WorkflowTemplates {
    /// Fetch → Extract → Store pipeline.
    pub fn fetch_extract_store(
        url: &str,
        schema_name: &str,
        namespace: &str,
    ) -> Result<Workflow, WorkflowError> {
        let mut builder = WorkflowBuilder::new("fetch-extract-store")
            .description("Fetch a URL, extract structured data, and store results");

        let fetch = builder.add_step(
            WorkflowStep::new("fetch", StepKind::Fetch { url: url.to_string() })
                .with_timeout(30),
        );
        let extract = builder.add_step(
            WorkflowStep::new(
                "extract",
                StepKind::Extract {
                    schema_name: schema_name.to_string(),
                },
            )
            .with_timeout(10),
        );
        let store = builder.add_step(
            WorkflowStep::new(
                "store",
                StepKind::Store {
                    namespace: namespace.to_string(),
                },
            )
            .with_timeout(5),
        );

        builder.add_edge(fetch, extract);
        builder.add_edge(extract, store);
        builder.build()
    }

    /// Parallel fetch → merge → transform pipeline.
    pub fn parallel_fetch_merge(urls: &[&str]) -> Result<Workflow, WorkflowError> {
        let mut builder = WorkflowBuilder::new("parallel-fetch-merge")
            .description("Fetch multiple URLs in parallel, then merge results");

        let fetches: Vec<usize> = urls
            .iter()
            .enumerate()
            .map(|(i, url)| {
                builder.add_step(
                    WorkflowStep::new(
                        format!("fetch-{i}"),
                        StepKind::Fetch { url: url.to_string() },
                    )
                    .with_timeout(30),
                )
            })
            .collect();

        let merge = builder.add_step(WorkflowStep::new("merge", StepKind::FanIn));

        for &fetch_idx in &fetches {
            builder.add_edge(fetch_idx, merge);
        }

        builder.build()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_builder_basic() {
        let mut builder = WorkflowBuilder::new("test");
        let a = builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        let b = builder.add_step(WorkflowStep::new("b", StepKind::FanIn));
        builder.add_edge(a, b);
        let wf = builder.build().unwrap();
        assert_eq!(wf.name, "test");
        assert_eq!(wf.steps.len(), 2);
        assert_eq!(wf.edges, vec![(0, 1)]);
        assert_eq!(wf.status, WorkflowStatus::Draft);
        assert!(!wf.definition_hash.is_empty());
    }

    #[test]
    fn test_workflow_builder_empty_fails() {
        let builder = WorkflowBuilder::new("empty");
        assert!(matches!(builder.build(), Err(WorkflowError::EmptyWorkflow)));
    }

    #[test]
    fn test_workflow_builder_invalid_edge() {
        let mut builder = WorkflowBuilder::new("bad");
        builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        builder.add_edge(0, 99);
        assert!(matches!(
            builder.build(),
            Err(WorkflowError::InvalidEdge { from: 0, to: 99 })
        ));
    }

    #[test]
    fn test_workflow_builder_cycle_detected() {
        let mut builder = WorkflowBuilder::new("cycle");
        let a = builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        let b = builder.add_step(WorkflowStep::new("b", StepKind::FanIn));
        builder.add_edge(a, b);
        builder.add_edge(b, a);
        assert!(matches!(
            builder.build(),
            Err(WorkflowError::CycleDetected)
        ));
    }

    #[test]
    fn test_execution_order_linear() {
        let mut builder = WorkflowBuilder::new("linear");
        let a = builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        let b = builder.add_step(WorkflowStep::new("b", StepKind::FanIn));
        let c = builder.add_step(WorkflowStep::new("c", StepKind::FanIn));
        builder.add_edge(a, b);
        builder.add_edge(b, c);
        let wf = builder.build().unwrap();
        let order = wf.execution_order().unwrap();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn test_execution_order_diamond() {
        let mut builder = WorkflowBuilder::new("diamond");
        let a = builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        let b = builder.add_step(WorkflowStep::new("b", StepKind::FanIn));
        let c = builder.add_step(WorkflowStep::new("c", StepKind::FanIn));
        let d = builder.add_step(WorkflowStep::new("d", StepKind::FanIn));
        builder.add_edge(a, b);
        builder.add_edge(a, c);
        builder.add_edge(b, d);
        builder.add_edge(c, d);
        let wf = builder.build().unwrap();
        let order = wf.execution_order().unwrap();
        // a must come first, d must come last
        assert_eq!(order[0], 0);
        assert_eq!(order[3], 3);
    }

    #[test]
    fn test_ready_steps_initial() {
        let mut builder = WorkflowBuilder::new("ready");
        let a = builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        let b = builder.add_step(WorkflowStep::new("b", StepKind::FanIn));
        let c = builder.add_step(WorkflowStep::new("c", StepKind::FanIn));
        builder.add_edge(a, c);
        builder.add_edge(b, c);
        let wf = builder.build().unwrap();
        let ready = wf.ready_steps();
        // a and b are ready (no deps), c is not
        assert!(ready.contains(&0));
        assert!(ready.contains(&1));
        assert!(!ready.contains(&2));
    }

    #[test]
    fn test_engine_start_workflow() {
        let mut engine = WorkflowEngine::new();
        let mut builder = WorkflowBuilder::new("test")
            .initial_input(serde_json::json!({"url": "https://example.com"}));
        let a = builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        let b = builder.add_step(WorkflowStep::new("b", StepKind::FanIn));
        builder.add_edge(a, b);
        let wf = builder.build().unwrap();
        let id = engine.register(wf);

        let ready = engine.start(&id).unwrap();
        assert_eq!(ready, vec![0]); // Only 'a' is ready

        let wf = engine.get(&id).unwrap();
        assert_eq!(wf.status, WorkflowStatus::Running);
        assert_eq!(wf.steps[0].status, StepStatus::Ready);
        assert_eq!(wf.steps[1].status, StepStatus::Pending);
        // Initial input propagated to source step
        assert!(wf.steps[0].input.is_some());
    }

    #[test]
    fn test_engine_complete_step_propagates() {
        let mut engine = WorkflowEngine::new();
        let mut builder = WorkflowBuilder::new("prop");
        let a = builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        let b = builder.add_step(WorkflowStep::new("b", StepKind::FanIn));
        builder.add_edge(a, b);
        let wf = builder.build().unwrap();
        let id = engine.register(wf);

        engine.start(&id).unwrap();
        engine.start_step(&id, 0, None).unwrap();

        let output = serde_json::json!({"html": "<h1>Hello</h1>"});
        let newly_ready = engine.complete_step(&id, 0, output.clone()).unwrap();
        assert_eq!(newly_ready, vec![1]); // b is now ready

        let wf = engine.get(&id).unwrap();
        assert_eq!(wf.steps[0].status, StepStatus::Completed);
        assert_eq!(wf.steps[1].status, StepStatus::Ready);
        // Output propagated as input to downstream step
        assert_eq!(wf.steps[1].input, Some(output));
    }

    #[test]
    fn test_engine_full_workflow_lifecycle() {
        let mut engine = WorkflowEngine::new();
        let mut builder = WorkflowBuilder::new("full");
        let a = builder.add_step(WorkflowStep::new("fetch", StepKind::Fetch { url: "https://example.com".into() }));
        let b = builder.add_step(WorkflowStep::new("extract", StepKind::Extract { schema_name: "product".into() }));
        let c = builder.add_step(WorkflowStep::new("store", StepKind::Store { namespace: "products".into() }));
        builder.add_edge(a, b);
        builder.add_edge(b, c);
        let wf = builder.build().unwrap();
        let id = engine.register(wf);

        // Start
        engine.start(&id).unwrap();
        assert_eq!(engine.get(&id).unwrap().status, WorkflowStatus::Running);

        // Execute step a
        engine.start_step(&id, 0, None).unwrap();
        engine.complete_step(&id, 0, serde_json::json!({"html": "..."})).unwrap();

        // Execute step b
        engine.start_step(&id, 1, None).unwrap();
        engine.complete_step(&id, 1, serde_json::json!({"data": {"title": "Widget"}})).unwrap();

        // Execute step c
        engine.start_step(&id, 2, None).unwrap();
        engine.complete_step(&id, 2, serde_json::json!({"stored": true})).unwrap();

        // Workflow should be complete
        let wf = engine.get(&id).unwrap();
        assert_eq!(wf.status, WorkflowStatus::Completed);
        assert!(wf.completed_at.is_some());
    }

    #[test]
    fn test_engine_fail_step_retry() {
        let mut engine = WorkflowEngine::new();
        let mut builder = WorkflowBuilder::new("retry");
        builder.add_step(WorkflowStep::new("a", StepKind::FanIn).with_retries(2));
        let wf = builder.build().unwrap();
        let id = engine.register(wf);

        engine.start(&id).unwrap();
        engine.start_step(&id, 0, None).unwrap();

        // First failure — should retry
        let retried = engine.fail_step(&id, 0, "timeout".into()).unwrap();
        assert!(retried);
        assert_eq!(engine.get(&id).unwrap().steps[0].status, StepStatus::Ready);

        // Second attempt
        engine.start_step(&id, 0, None).unwrap();
        let retried = engine.fail_step(&id, 0, "timeout again".into()).unwrap();
        assert!(!retried); // max_retries=2, attempt=2 → permanent failure
        assert_eq!(engine.get(&id).unwrap().steps[0].status, StepStatus::Failed);
        assert_eq!(engine.get(&id).unwrap().status, WorkflowStatus::Failed);
    }

    #[test]
    fn test_engine_cancel_workflow() {
        let mut engine = WorkflowEngine::new();
        let mut builder = WorkflowBuilder::new("cancel");
        builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        builder.add_step(WorkflowStep::new("b", StepKind::FanIn));
        let wf = builder.build().unwrap();
        let id = engine.register(wf);

        engine.start(&id).unwrap();
        engine.cancel(&id).unwrap();

        let wf = engine.get(&id).unwrap();
        assert_eq!(wf.status, WorkflowStatus::Cancelled);
        assert!(wf.steps.iter().all(|s| s.status == StepStatus::Cancelled));
    }

    #[test]
    fn test_engine_pause_and_resume() {
        let mut engine = WorkflowEngine::new();
        let mut builder = WorkflowBuilder::new("pause");
        builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        let wf = builder.build().unwrap();
        let id = engine.register(wf);

        engine.start(&id).unwrap();
        engine.pause(&id).unwrap();
        assert_eq!(engine.get(&id).unwrap().status, WorkflowStatus::Paused);

        // Resume via start
        engine.start(&id).unwrap();
        assert_eq!(engine.get(&id).unwrap().status, WorkflowStatus::Running);
    }

    #[test]
    fn test_engine_event_log() {
        let mut engine = WorkflowEngine::new();
        let mut builder = WorkflowBuilder::new("events");
        builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        let wf = builder.build().unwrap();
        let id = engine.register(wf);

        engine.start(&id).unwrap();
        engine.start_step(&id, 0, None).unwrap();
        engine.complete_step(&id, 0, serde_json::json!({})).unwrap();

        let events = engine.events_for(&id);
        assert!(events.len() >= 4); // started, step_ready, step_started, step_completed, workflow_completed
        assert_eq!(events[0].kind, WorkflowEventKind::WorkflowStarted);
    }

    #[test]
    fn test_engine_not_found() {
        let mut engine = WorkflowEngine::new();
        let fake_id = Uuid::new_v4();
        assert!(matches!(
            engine.start(&fake_id),
            Err(WorkflowError::NotFound(_))
        ));
    }

    #[test]
    fn test_engine_list_workflows() {
        let mut engine = WorkflowEngine::new();
        let mut b1 = WorkflowBuilder::new("wf1");
        b1.add_step(WorkflowStep::new("a", StepKind::FanIn));
        let mut b2 = WorkflowBuilder::new("wf2");
        b2.add_step(WorkflowStep::new("b", StepKind::FanIn));
        engine.register(b1.build().unwrap());
        engine.register(b2.build().unwrap());
        assert_eq!(engine.list().len(), 2);
    }

    #[test]
    fn test_step_counts() {
        let mut builder = WorkflowBuilder::new("counts");
        builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        builder.add_step(WorkflowStep::new("b", StepKind::FanIn));
        builder.add_step(WorkflowStep::new("c", StepKind::FanIn));
        let wf = builder.build().unwrap();
        let counts = wf.step_counts();
        assert_eq!(counts.get(&StepStatus::Pending), Some(&3));
    }

    #[test]
    fn test_is_terminal() {
        let mut builder = WorkflowBuilder::new("term");
        builder.add_step(WorkflowStep::new("a", StepKind::FanIn));
        let mut wf = builder.build().unwrap();
        assert!(!wf.is_terminal());
        wf.status = WorkflowStatus::Completed;
        assert!(wf.is_terminal());
        wf.status = WorkflowStatus::Failed;
        assert!(wf.is_terminal());
        wf.status = WorkflowStatus::Cancelled;
        assert!(wf.is_terminal());
    }

    #[test]
    fn test_parallel_steps_both_ready() {
        let mut engine = WorkflowEngine::new();
        let mut builder = WorkflowBuilder::new("parallel");
        let a = builder.add_step(WorkflowStep::new("source", StepKind::FanIn));
        let b = builder.add_step(WorkflowStep::new("branch1", StepKind::FanIn));
        let c = builder.add_step(WorkflowStep::new("branch2", StepKind::FanIn));
        let d = builder.add_step(WorkflowStep::new("merge", StepKind::FanIn));
        builder.add_edge(a, b);
        builder.add_edge(a, c);
        builder.add_edge(b, d);
        builder.add_edge(c, d);
        let wf = builder.build().unwrap();
        let id = engine.register(wf);

        engine.start(&id).unwrap();
        engine.start_step(&id, 0, None).unwrap();
        let ready = engine.complete_step(&id, 0, serde_json::json!({})).unwrap();
        // Both branches should be ready
        assert!(ready.contains(&1));
        assert!(ready.contains(&2));
        // Merge should not be ready yet
        assert!(!ready.contains(&3));

        // Complete branch1
        engine.start_step(&id, 1, None).unwrap();
        let ready2 = engine.complete_step(&id, 1, serde_json::json!({})).unwrap();
        // Merge still not ready (branch2 incomplete)
        assert!(!ready2.contains(&3));

        // Complete branch2
        engine.start_step(&id, 2, None).unwrap();
        let ready3 = engine.complete_step(&id, 2, serde_json::json!({})).unwrap();
        // Now merge should be ready
        assert!(ready3.contains(&3));
    }

    #[test]
    fn test_template_fetch_extract_store() {
        let wf =
            WorkflowTemplates::fetch_extract_store("https://example.com", "product", "products")
                .unwrap();
        assert_eq!(wf.steps.len(), 3);
        assert_eq!(wf.edges.len(), 2);
        let order = wf.execution_order().unwrap();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn test_template_parallel_fetch_merge() {
        let wf =
            WorkflowTemplates::parallel_fetch_merge(&["https://a.com", "https://b.com", "https://c.com"])
                .unwrap();
        assert_eq!(wf.steps.len(), 4); // 3 fetches + 1 merge
        assert_eq!(wf.edges.len(), 3);
    }

    #[test]
    fn test_step_with_builders() {
        let step = WorkflowStep::new("test", StepKind::FanIn)
            .with_timeout(60)
            .with_retries(5)
            .with_capabilities(vec!["web".into()]);
        assert_eq!(step.timeout_secs, Some(60));
        assert_eq!(step.max_retries, 5);
        assert_eq!(step.required_capabilities, vec!["web"]);
    }

    #[test]
    fn test_workflow_serde_roundtrip() {
        let mut builder = WorkflowBuilder::new("serde");
        let a = builder.add_step(WorkflowStep::new("a", StepKind::Fetch { url: "https://x.com".into() }));
        let b = builder.add_step(WorkflowStep::new("b", StepKind::Extract { schema_name: "s".into() }));
        builder.add_edge(a, b);
        let wf = builder.build().unwrap();
        let json = serde_json::to_string(&wf).unwrap();
        let back: Workflow = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "serde");
        assert_eq!(back.steps.len(), 2);
    }

    #[test]
    fn test_step_kind_variants() {
        let kinds = vec![
            StepKind::Fetch { url: "u".into() },
            StepKind::Extract { schema_name: "s".into() },
            StepKind::Transform { transformer: "t".into() },
            StepKind::Store { namespace: "n".into() },
            StepKind::AgentCall { capability: "c".into() },
            StepKind::Script { source: "x".into() },
            StepKind::FanOut { branches: 3 },
            StepKind::FanIn,
            StepKind::Conditional { predicate_key: "k".into() },
            StepKind::Custom { kind: "k".into(), config: serde_json::json!(null) },
        ];
        for kind in kinds {
            let json = serde_json::to_string(&kind).unwrap();
            let back: StepKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }
}
