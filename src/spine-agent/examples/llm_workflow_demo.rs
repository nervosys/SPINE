//! # SPINE LLM Workflow Demo
//!
//! End-to-end LLM-powered agent workflow with pluggable LLM dispatcher.

use spine_agentic::workflow::{
    StepKind, WorkflowBuilder, WorkflowEngine, WorkflowStep, StepStatus,
};
use spine_compiler::Compiler;
use spine_parser::parse_html;

enum LlmBackend { Offline, OpenAi(String), Anthropic(String) }

struct LlmDispatcher { backend: LlmBackend }

impl LlmDispatcher {
    fn auto() -> Self {
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            if !key.is_empty() {
                println!("  [LLM] Using OpenAI backend");
                return Self { backend: LlmBackend::OpenAi(key) };
            }
        }
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            if !key.is_empty() {
                println!("  [LLM] Using Anthropic backend");
                return Self { backend: LlmBackend::Anthropic(key) };
            }
        }
        println!("  [LLM] No API key - using offline backend");
        Self { backend: LlmBackend::Offline }
    }

    fn complete(&self, prompt: &str) -> String {
        match &self.backend {
            LlmBackend::Offline => offline_complete(prompt),
            LlmBackend::OpenAi(key) => {
                println!("  [OpenAI] Would call API (key={}...)", &key[..key.len().min(8)]);
                offline_complete(prompt)
            }
            LlmBackend::Anthropic(key) => {
                println!("  [Anthropic] Would call API (key={}...)", &key[..key.len().min(8)]);
                offline_complete(prompt)
            }
        }
    }
}

fn offline_complete(prompt: &str) -> String {
    if prompt.contains("summarize") {
        "SPINE: 28-crate bioinspired agentic web stack with latent-space crypto.".into()
    } else if prompt.contains("extract") {
        "Entities: [SPINE, Chameleon Protocol, Titans Memory, RLWE]".into()
    } else if prompt.contains("classify") {
        "Category: AI Infrastructure".into()
    } else {
        format!("Offline: {}", &prompt[..prompt.len().min(50)])
    }
}

fn main() {
    println!("=== SPINE LLM Workflow Demo ===\n");
    let llm = LlmDispatcher::auto();

    // Build workflow DAG
    let mut builder = WorkflowBuilder::new("llm-research")
        .description("LLM-Powered Research Pipeline");

    let parse_idx = builder.add_step(WorkflowStep::new("parse", StepKind::Custom {
        kind: "parse".into(), config: serde_json::json!({})
    }));
    let summarize_idx = builder.add_step(WorkflowStep::new("summarize", StepKind::AgentCall {
        capability: "llm-summarize".into()
    }));
    let extract_idx = builder.add_step(WorkflowStep::new("extract", StepKind::AgentCall {
        capability: "llm-extract".into()
    }));
    let classify_idx = builder.add_step(WorkflowStep::new("classify", StepKind::AgentCall {
        capability: "llm-classify".into()
    }));
    let aggregate_idx = builder.add_step(WorkflowStep::new("aggregate", StepKind::FanIn));
    let compile_idx = builder.add_step(WorkflowStep::new("compile", StepKind::Script {
        source: "element Report { }".into()
    }));

    builder.add_edge(parse_idx, summarize_idx);
    builder.add_edge(parse_idx, extract_idx);
    builder.add_edge(parse_idx, classify_idx);
    builder.add_edge(summarize_idx, aggregate_idx);
    builder.add_edge(extract_idx, aggregate_idx);
    builder.add_edge(classify_idx, aggregate_idx);
    builder.add_edge(aggregate_idx, compile_idx);

    let workflow = builder.build().expect("valid workflow");
    println!("Workflow: {} steps, {} edges", workflow.steps.len(), workflow.edges.len());

    let mut engine = WorkflowEngine::new();
    let wf_id = engine.register(workflow);
    engine.start(&wf_id).expect("start");

    // Parse HTML
    let html = r#"<html><head><title>SPINE</title></head><body>
        <h1>Architecture</h1><p>28 crates for agentic web.</p>
        <ul><li>Chameleon Protocol</li><li>Titans Memory</li></ul>
    </body></html>"#;
    let ur = parse_html(html).expect("parse");
    println!("Parsed: {} elements", ur.elements.len());
    engine.start_step(&wf_id, parse_idx, None).unwrap();
    engine.complete_step(&wf_id, parse_idx, serde_json::json!({"ok": true})).unwrap();

    let text = format!("{:?}", ur.elements);

    // LLM steps
    engine.start_step(&wf_id, summarize_idx, None).unwrap();
    let summary = llm.complete(&format!("Please summarize: {}", text));
    println!("Summary: {}", summary);
    engine.complete_step(&wf_id, summarize_idx, serde_json::json!({"s": summary})).unwrap();

    engine.start_step(&wf_id, extract_idx, None).unwrap();
    let entities = llm.complete(&format!("Extract entities: {}", text));
    println!("Entities: {}", entities);
    engine.complete_step(&wf_id, extract_idx, serde_json::json!({"e": entities})).unwrap();

    engine.start_step(&wf_id, classify_idx, None).unwrap();
    let cat = llm.complete(&format!("Classify: {}", text));
    println!("Category: {}", cat);
    engine.complete_step(&wf_id, classify_idx, serde_json::json!({"c": cat})).unwrap();

    engine.start_step(&wf_id, aggregate_idx, None).unwrap();
    engine.complete_step(&wf_id, aggregate_idx, serde_json::json!({"merged": true})).unwrap();

    // Compile HLS
    let hls = "let title = \"Report\"\nelement Report { element Body { } }";
    let report = match Compiler::compile(hls) {
        Ok(b) => format!("SpineBinary({} bytes)", b.instructions.len()),
        Err(e) => format!("Note: {:?}", e),
    };
    println!("Report: {}", report);
    engine.start_step(&wf_id, compile_idx, None).unwrap();
    engine.complete_step(&wf_id, compile_idx, serde_json::json!({"r": report})).unwrap();

    if let Some(wf) = engine.get(&wf_id) {
        println!("\nStatus: {:?}", wf.status);
        println!("Completed: {}", wf.step_counts().get(&StepStatus::Completed).unwrap_or(&0));
    }
    println!("Events: {}", engine.all_events().len());
    println!("\nDone.");
}