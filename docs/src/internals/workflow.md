# Workflow Orchestration

SPINE provides DAG-based multi-step agent pipelines for coordinating complex tasks.

## Overview

A **Workflow** is a directed acyclic graph of **WorkflowSteps**, where each step performs an action (fetch, extract, transform, call another agent, run a script, etc.). The `WorkflowEngine` executes workflows, tracking step status, emitting events, and enforcing dependency ordering.

## Core Types

| Type | Purpose |
|------|---------|
| `WorkflowBuilder` | Fluent API for constructing workflow DAGs with cycle detection |
| `Workflow` | Immutable DAG with steps, edges, and definition hash (SHA-256) |
| `WorkflowStep` | Named step with a `StepKind` and description |
| `WorkflowEngine` | Execution engine with start/complete/fail/cancel/pause operations |
| `StepKind` | 10 variants: Fetch, Extract, Transform, Store, AgentCall, Script, FanOut, FanIn, Conditional, Custom |

## Step Kinds

| Variant | Description |
|---------|-------------|
| `Fetch { url }` | HTTP fetch from a URL |
| `Extract { schema_name }` | Apply an ExtractionSchema |
| `Transform { transformer }` | Run a named transformer |
| `Store { namespace }` | Persist to a namespace |
| `AgentCall { capability }` | Delegate to another agent by capability |
| `Script { source }` | Execute inline HLS script |
| `FanOut { branches }` | Fork into N parallel branches |
| `FanIn` | Join parallel branches |
| `Conditional { predicate_key }` | Branch based on a predicate |
| `Custom { kind, config }` | User-defined step type |

## Templates

Six built-in workflow templates are available:
`web_scraping_pipeline`, `data_enrichment`, `content_analysis`, `monitoring`, `agent_delegation`, `etl_pipeline`.

## Example

```rust,ignore
let workflow = WorkflowBuilder::new("research")
    .description("Web research pipeline")
    .add_step(WorkflowStep::new("fetch", StepKind::Fetch { url: "https://example.com".into() }))
    .add_step(WorkflowStep::new("extract", StepKind::Extract { schema_name: "article".into() }))
    .add_edge(0, 1)
    .build()?;

let engine = WorkflowEngine::new();
let id = engine.register(workflow);
engine.start(&id)?;
```