// Allow fire-and-forget async patterns for distributed operations
#![allow(clippy::let_underscore_future)]

use anyhow::{Context, Result};
use axum::{routing::get, Router};
use dashmap::DashMap;
use prometheus::{Encoder, TextEncoder};
use serde::{Deserialize, Serialize};
use spine_cluster::{ClusterNode, NodeCapabilities};
use spine_neural::NeuralLatentEncoder;
use spine_parser::parse_html;
use spine_protocol::{BrowserCommand, Message, PreComputedResponse, ProtocolHandler, Response};
use spine_transport::WebSocketStream;
use spine_wasm::WasmRuntime;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::{debug, error, info, instrument, span, warn, Level};
use uuid::Uuid;

mod config;
#[allow(dead_code)]
mod ct;
mod telemetry;
#[allow(dead_code)]
mod tls;
mod vdom;
use config::SpineConfig;
use telemetry::*;
use tls::*;
use vdom::VirtualDom;

#[derive(Serialize, Deserialize, Clone)]
struct Session {
    current_url: Option<String>,
    current_html: Option<String>,
    /// Cached VDOM from last ExecuteBinary
    current_vdom: Option<VirtualDom>,
    /// Cached parsed UnifiedRepresentation (invalidated on navigation)
    #[serde(skip)]
    cached_ur: Option<spine_parser::UnifiedRepresentation>,
    last_command: Option<BrowserCommand>,
    needs_morph: bool,
    /// Reactive state variables
    state: std::collections::HashMap<String, serde_json::Value>,
    /// Current HLB binary for reactivity
    current_binary: Option<spine_protocol::SpineBinary>,
    /// Command history for this session
    history: Vec<BrowserCommand>,
    /// Whether the agent is in autonomous mode
    autonomous_mode: bool,
}

impl Session {
    fn new() -> Self {
        Self {
            current_url: None,
            current_html: None,
            current_vdom: None,
            cached_ur: None,
            last_command: None,
            needs_morph: false,
            state: std::collections::HashMap::new(),
            current_binary: None,
            history: Vec::new(),
            autonomous_mode: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct KnowledgeEntry {
    key: String,
    value: serde_json::Value,
    tags: Vec<String>,
    timestamp_ns: u64,
}

#[derive(Clone)]
struct KnowledgeProposal {
    key: String,
    value: serde_json::Value,
    tags: Vec<String>,
    votes: Vec<KnowledgeVote>,
    origin_node: spine_cluster::NodeId,
}

#[derive(Clone)]
struct KnowledgeVote {
    voter_id: spine_cluster::NodeId,
    approved: bool,
    confidence: f32,
}

struct BrowserState {
    sessions: DashMap<String, Session>,
    knowledge_base: DashMap<String, Vec<KnowledgeEntry>>,
    proposals: DashMap<Uuid, KnowledgeProposal>,
    plans: DashMap<Uuid, spine_protocol::SwarmPlan>,
    client: reqwest::Client,
    /// RwLock allows concurrent encoder reads (multiple sessions encoding in parallel)
    encoder: tokio::sync::RwLock<NeuralLatentEncoder>,
    cluster: Mutex<ClusterNode>,
    agentic_runtime: Arc<spine_agentic::AgenticWebRuntime>,
    /// Rate limiter: session_id -> (tokens, last_update)
    rate_limits: DashMap<String, (f64, std::time::Instant)>,
    /// Cached WasmRuntime (expensive to create, safe to reuse)
    wasm_runtime: spine_wasm::WasmRuntime,
    /// Cached NeuralProtocol per domain (avoids re-allocating neural weights per request)
    neural_protocols: DashMap<String, spine_agentic::NeuralProtocol>,
    /// Active connection count per IP address
    connections_per_ip: DashMap<std::net::IpAddr, std::sync::atomic::AtomicUsize>,
    /// Total active connection count
    active_connections: std::sync::atomic::AtomicUsize,
    /// Server start time for uptime calculation
    start_time: std::time::Instant,
    /// Shutdown signal: when true, stop accepting new connections
    shutting_down: std::sync::atomic::AtomicBool,
    /// Server configuration
    config: SpineConfig,
}

impl BrowserState {
    async fn save_sessions(&self) -> anyhow::Result<()> {
        let dir = std::path::Path::new("sessions");
        tokio::fs::create_dir_all(dir).await.ok();

        for entry in self.sessions.iter() {
            let id = entry.key();
            let session = entry.value();
            let path = dir.join(format!("{}.json", id));
            let json = serde_json::to_string(session)?;
            tokio::fs::write(path, json).await?;
        }

        // Save knowledge base
        let kb_dir = std::path::Path::new("knowledge");
        tokio::fs::create_dir_all(kb_dir).await.ok();
        for entry in self.knowledge_base.iter() {
            let id = entry.key();
            let entries = entry.value();
            let path = kb_dir.join(format!("{}.json", id));
            let json = serde_json::to_string(entries)?;
            tokio::fs::write(path, json).await?;
        }

        Ok(())
    }

    fn load_sessions(&self) -> anyhow::Result<()> {
        let dir = std::path::Path::new("sessions");
        if dir.exists() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(String::from) {
                        let json = std::fs::read_to_string(&path)?;
                        let session: Session = serde_json::from_str(&json)?;
                        self.sessions.insert(id, session);
                    }
                }
            }
        }

        let kb_dir = std::path::Path::new("knowledge");
        if kb_dir.exists() {
            for entry in std::fs::read_dir(kb_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(String::from) {
                        let json = std::fs::read_to_string(&path)?;
                        let entries: Vec<KnowledgeEntry> = serde_json::from_str(&json)?;
                        self.knowledge_base.insert(id, entries);
                    }
                }
            }
        }
        Ok(())
    }

    fn check_rate_limit(&self, session_id: &str) -> bool {
        let max_tokens = 100.0;
        let refill_rate = 10.0; // tokens per second

        let mut entry = self
            .rate_limits
            .entry(session_id.to_string())
            .or_insert((max_tokens, std::time::Instant::now()));
        let (tokens, last_update) = entry.value_mut();

        let now = std::time::Instant::now();
        let elapsed = now.duration_since(*last_update).as_secs_f64();
        *tokens = (*tokens + elapsed * refill_rate).min(max_tokens);
        *last_update = now;

        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[instrument(skip(state, session_id, request_id))]
async fn handle_command(
    state: &Arc<BrowserState>,
    session_id: &str,
    command: BrowserCommand,
    request_id: String,
) -> (Response, Vec<Vec<f32>>) {
    let timer = COMMAND_LATENCY.start_timer();
    COMMANDS_TOTAL.inc();

    // Record command in history
    if let Some(mut session) = state.sessions.get_mut(session_id) {
        session.history.push(command.clone());
        session.last_command = Some(command.clone());
    }

    let mut latent_to_stream = Vec::new();
    let response = match command {
        BrowserCommand::Navigate { url } => {
            match state.client.get(&url).send().await {
                Ok(resp) => {
                    let html = resp.text().await.unwrap_or_default();

                    // Transpile HTML to HLB for agentic interaction
                    let hlb = spine_human::HumanTranspiler::transpile(&html, "", "")
                        .unwrap_or_else(|e| {
                            warn!("Transpilation failed for {}: {}", url, e);
                            spine_protocol::SpineBinary {
                                instructions: vec![],
                                data: vec![],
                                render_start: 0,
                                exported_functions: std::collections::HashMap::new(),
                                capabilities: vec![],
                            }
                        });

                    if let Some(mut session) = state.sessions.get_mut(session_id) {
                        session.current_url = Some(url);
                        session.current_html = Some(html);
                        session.current_binary = Some(hlb);
                        session.current_vdom = None;
                        session.cached_ur = None; // Invalidate cached UR on navigation
                    } else {
                        // Create new session if it doesn't exist
                        let session = Session {
                            current_url: Some(url),
                            current_html: Some(html),
                            current_vdom: None,
                            cached_ur: None,
                            last_command: None,
                            needs_morph: false,
                            state: std::collections::HashMap::new(),
                            current_binary: Some(hlb),
                            history: Vec::new(),
                            autonomous_mode: false,
                        };
                        state.sessions.insert(session_id.to_string(), session);
                    }
                    Response {
                        id: request_id,
                        result: Some(
                            serde_json::json!({"status": "success", "session_id": session_id}),
                        ),
                        error: None,
                    }
                }
                Err(e) => Response {
                    id: request_id,
                    result: None,
                    error: Some(e.to_string()),
                },
            }
        }
        BrowserCommand::GetUR => {
            if let Some(mut session) = state.sessions.get_mut(session_id) {
                // Populate cache on first access (avoids re-parsing on repeated GetUR calls)
                if session.cached_ur.is_none() {
                    if let Some(html) = session.current_html.clone() {
                        session.cached_ur = spine_parser::parse_html(&html).ok();
                    }
                }
                if let Some(ur) = &session.cached_ur {
                    let vdom_tree = session.current_vdom.as_ref().map(|v| v.to_tree());
                    let vdom_ur = session.current_vdom.as_ref().map(|v| v.to_ur());
                    Response {
                        id: request_id,
                        result: Some(serde_json::json!({
                            "parsed_ur": serde_json::to_value(ur).unwrap_or_default(),
                            "vdom_tree": vdom_tree,
                            "vdom_ur": vdom_ur,
                        })),
                        error: None,
                    }
                } else {
                    Response {
                        id: request_id,
                        result: None,
                        error: Some("No page loaded in session".to_string()),
                    }
                }
            } else {
                Response {
                    id: request_id,
                    result: None,
                    error: Some("Session not found".to_string()),
                }
            }
        }
        BrowserCommand::GetRawHTML => {
            if let Some(session) = state.sessions.get(session_id) {
                if let Some(html) = &session.current_html {
                    Response {
                        id: request_id,
                        result: Some(serde_json::json!({ "html": html })),
                        error: None,
                    }
                } else {
                    Response {
                        id: request_id,
                        result: None,
                        error: Some("No page loaded".to_string()),
                    }
                }
            } else {
                Response {
                    id: request_id,
                    result: None,
                    error: Some("Session not found".to_string()),
                }
            }
        }
        BrowserCommand::ExecuteBinary(bin) => {
            info!(
                "Executing HLB with {} instructions and capabilities: {:?}",
                bin.instructions.len(),
                bin.capabilities
            );

            // Capability check
            for cap in &bin.capabilities {
                match cap.as_str() {
                    "network" | "storage" | "memory" | "search" => {
                        // Allowed by default for now
                    }
                    _ => {
                        return (
                            Response {
                                id: request_id,
                                result: None,
                                error: Some(format!("Unauthorized or unknown capability: {}", cap)),
                            },
                            Vec::new(),
                        );
                    }
                }
            }

            // Store binary in session for reactivity
            if let Some(mut session) = state.sessions.get_mut(session_id) {
                session.current_binary = Some(bin.clone());
            }

            let result = state
                .wasm_runtime
                .execute(&bin)
                .expect("Failed to execute HLB in WASM");

            latent_to_stream = result.latent_streams.clone();

            // Update session VDOM and compute patches
            let new_vdom = VirtualDom::from_wasm(&result);
            let mut patches = Vec::new();

            if let Some(mut session) = state.sessions.get_mut(session_id) {
                if let Some(old_vdom) = &session.current_vdom {
                    patches = new_vdom.diff(old_vdom);
                }
                session.current_vdom = Some(new_vdom);

                // Handle state events from WASM
                for event in &result.events {
                    match event.name.as_str() {
                        "state_declared" | "state_updated" => {
                            if let (Some(name), Some(value)) = (
                                event.payload.get("name").and_then(|v| v.as_str()),
                                event.payload.get("value"),
                            ) {
                                session.state.insert(name.to_string(), value.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Handle agentic actions from WASM
            for action in &result.actions {
                match action {
                    spine_wasm::WasmAction::Navigate(url) => {
                        info!("WASM requested navigation to {}", url);
                        let state_clone = state.clone();
                        let url_clone = url.clone();
                        let sid_clone = session_id.to_string();
                        tokio::spawn(async move {
                            if let Ok(resp) = state_clone.client.get(&url_clone).send().await {
                                if let Ok(html) = resp.text().await {
                                    if let Some(mut session) =
                                        state_clone.sessions.get_mut(&sid_clone)
                                    {
                                        session.current_url = Some(url_clone);
                                        session.current_html = Some(html);
                                    }
                                }
                            }
                        });
                    }
                    spine_wasm::WasmAction::Search(query) => {
                        info!("WASM requested search for {}", query);
                        let cluster = state.cluster.lock().await;
                        let _ = cluster.broadcast_search(query.clone(), request_id.clone());
                    }
                    spine_wasm::WasmAction::StoreKnowledge { key, value, tags } => {
                        info!("WASM requested knowledge storage: {}", key);
                        if let Some(_session) = state.sessions.get_mut(session_id) {
                            // Store in local KB
                            let entry = KnowledgeEntry {
                                key: key.clone(),
                                value: value.clone(),
                                tags: tags.clone(),
                                timestamp_ns: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_nanos()
                                    as u64,
                            };
                            state
                                .knowledge_base
                                .entry(key.clone())
                                .or_default()
                                .push(entry);

                            let cluster = state.cluster.lock().await;
                            let _ = cluster.broadcast_knowledge(
                                key.clone(),
                                value.clone(),
                                tags.clone(),
                            );
                        }
                    }
                    spine_wasm::WasmAction::QueryKnowledge {
                        query,
                        tags: _,
                        limit: _,
                    } => {
                        info!("WASM requested knowledge query: {}", query);
                    }
                    spine_wasm::WasmAction::Reason(query) => {
                        info!("WASM requested reasoning: {}", query);
                        if let Some(mut session) = state.sessions.get_mut(session_id) {
                            if let Some(html) = &session.current_html {
                                if let Ok(ur) = spine_parser::parse_html(html) {
                                    let engine = spine_human::ReasoningEngine::new();
                                    let plan = engine.create_plan(query, &ur);
                                    info!("Reasoning plan: {:?}", plan);
                                    // Store the plan in session history or state for the agent to see
                                    session.history.push(BrowserCommand::Navigate {
                                        url: format!("reasoning://{}", query),
                                    });
                                }
                            }
                        }
                    }
                }
            }

            Response {
                id: request_id,
                result: Some(serde_json::json!({
                    "status": "executed",
                    "stats": {
                        "instructions_executed": result.stats.instructions_executed,
                        "compile_time_us": result.stats.compile_time_us,
                        "execution_time_us": result.stats.execution_time_us,
                        "wasm_size_bytes": result.stats.wasm_size_bytes,
                        "memory_used_bytes": result.stats.memory_used_bytes,
                    },
                    "elements": result.elements,
                    "events": result.events,
                    "actions": result.actions,
                    "latent_streams_count": result.latent_streams.len(),
                    "patches": patches,
                })),
                error: None,
            }
        }
        BrowserCommand::HandleEvent {
            element_id: _element_id,
            event_name,
            payload: _payload,
        } => {
            if let Some(mut session) = state.sessions.get_mut(session_id) {
                let bin = session.current_binary.clone();
                if let Some(bin) = bin {
                    if let Some(&pc) = bin.exported_functions.get(&event_name) {
                        let mut runtime = vdom::HlbRuntime::new();
                        // Load session state into runtime
                        for (k, v) in &session.state {
                            runtime.state.insert(k.clone(), v.clone());
                        }

                        // Execute handler
                        let _handler_result = runtime.execute(&bin, pc);

                        // Update session state from runtime
                        for (k, v) in &runtime.state {
                            session.state.insert(k.clone(), v.clone());
                        }

                        // Re-render from render_start
                        let render_result = runtime.execute(&bin, bin.render_start);

                        // Compute patches
                        let new_vdom = render_result.vdom;
                        let mut patches = Vec::new();
                        if let Some(old_vdom) = &session.current_vdom {
                            patches = new_vdom.diff(old_vdom);
                        }
                        session.current_vdom = Some(new_vdom);

                        Response {
                            id: request_id,
                            result: Some(serde_json::json!({
                                "status": "event_handled",
                                "patches": patches,
                                "events": render_result.events,
                            })),
                            error: None,
                        }
                    } else {
                        Response {
                            id: request_id,
                            result: None,
                            error: Some(format!("Handler not found for event: {}", event_name)),
                        }
                    }
                } else {
                    Response {
                        id: request_id,
                        result: None,
                        error: Some("No binary loaded in session".to_string()),
                    }
                }
            } else {
                Response {
                    id: request_id,
                    result: None,
                    error: Some("Session not found".to_string()),
                }
            }
        }
        BrowserCommand::Click { element_id } => {
            println!("Clicking element: {}", element_id);
            Response {
                id: request_id,
                result: Some(serde_json::json!({ "status": "clicked" })),
                error: None,
            }
        }
        BrowserCommand::Type { element_id, text } => {
            println!("Typing '{}' into element: {}", text, element_id);
            Response {
                id: request_id,
                result: Some(serde_json::json!({ "status": "typed" })),
                error: None,
            }
        }
        BrowserCommand::GetLatentUR { dimensions } => {
            if let Some(session) = state.sessions.get(session_id) {
                if let Some(html) = &session.current_html {
                    match spine_parser::parse_html(html) {
                        Ok(ur) => {
                            let ur_json = serde_json::to_string(&ur).unwrap_or_default();
                            let mut encoder = state.encoder.write().await;
                            let latent_vector = encoder.encode(ur_json.as_bytes());
                            Response {
                                id: request_id,
                                result: Some(serde_json::json!({
                                    "ur": ur,
                                    "latent_vector": latent_vector,
                                    "dimensions": dimensions,
                                    "status": "encoded"
                                })),
                                error: None,
                            }
                        }
                        Err(e) => Response {
                            id: request_id,
                            result: None,
                            error: Some(e.to_string()),
                        },
                    }
                } else {
                    Response {
                        id: request_id,
                        result: None,
                        error: Some("No page loaded".to_string()),
                    }
                }
            } else {
                Response {
                    id: request_id,
                    result: None,
                    error: Some("Session not found".to_string()),
                }
            }
        }
        BrowserCommand::Morph => {
            info!("Triggering protocol morphing for session {}", session_id);
            let mut encoder = state.encoder.write().await;
            let seed = rand::random::<u64>();
            encoder.evolve(seed);
            Response {
                id: request_id,
                result: Some(serde_json::json!({
                    "status": "morphed",
                    "new_seed": seed,
                    "protocol": "chameleon-v2"
                })),
                error: None,
            }
        }
        BrowserCommand::Search { query } => {
            if let Some(session) = state.sessions.get(session_id) {
                if let Some(_html) = &session.current_html {
                    // Local search
                    let local_results = serde_json::json!([
                        { "text": "Found relevant content locally...", "relevance": 0.92 }
                    ]);

                    // Broadcast to cluster for distributed search
                    let cluster = state.cluster.lock().await;
                    let _ = cluster.broadcast_search(query.clone(), request_id.clone());

                    Response {
                        id: request_id,
                        result: Some(serde_json::json!({
                            "query": query,
                            "local_results": local_results,
                            "distributed": true,
                            "status": "searching_cluster"
                        })),
                        error: None,
                    }
                } else {
                    Response {
                        id: request_id,
                        result: None,
                        error: Some("No page loaded".to_string()),
                    }
                }
            } else {
                Response {
                    id: request_id,
                    result: None,
                    error: Some("Session not found".to_string()),
                }
            }
        }
        BrowserCommand::TransferSession { target_node_id } => {
            if let Some(session) = state.sessions.get(session_id) {
                match serde_json::to_vec(&*session) {
                    Ok(data) => {
                        let cluster = state.cluster.lock().await;
                        if let Err(e) = cluster
                            .send_session_data(session_id.to_string(), data)
                            .await
                        {
                            Response {
                                id: request_id,
                                result: None,
                                error: Some(format!("Transfer failed: {}", e)),
                            }
                        } else {
                            Response {
                                id: request_id,
                                result: Some(
                                    serde_json::json!({ "status": "transferred", "target": target_node_id }),
                                ),
                                error: None,
                            }
                        }
                    }
                    Err(e) => Response {
                        id: request_id,
                        result: None,
                        error: Some(format!("Serialization failed: {}", e)),
                    },
                }
            } else {
                Response {
                    id: request_id,
                    result: None,
                    error: Some("Session not found".to_string()),
                }
            }
        }
        BrowserCommand::StoreKnowledge { key, value, tags } => {
            let entry = KnowledgeEntry {
                key: key.clone(),
                value: value.clone(),
                tags: tags.clone(),
                timestamp_ns: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
            };
            state
                .knowledge_base
                .entry(session_id.to_string())
                .or_default()
                .push(entry);

            // Broadcast to cluster
            let cluster = state.cluster.lock().await;
            let _ = cluster.broadcast_knowledge(key, value, tags);

            Response {
                id: request_id,
                result: Some(serde_json::json!({ "status": "knowledge_stored" })),
                error: None,
            }
        }
        BrowserCommand::QueryKnowledge { query, tags, limit } => {
            let entries = state
                .knowledge_base
                .get(session_id)
                .map(|e| e.clone())
                .unwrap_or_default();
            let results: Vec<_> = entries
                .into_iter()
                .filter(|e| {
                    let matches_tags = tags.is_empty() || tags.iter().all(|t| e.tags.contains(t));
                    let matches_query = query.is_empty() || e.key.contains(&query);
                    matches_tags && matches_query
                })
                .take(limit)
                .collect();
            Response {
                id: request_id,
                result: Some(serde_json::json!({ "results": results })),
                error: None,
            }
        }
        BrowserCommand::DeleteKnowledge { key } => {
            if let Some(mut entries) = state.knowledge_base.get_mut(session_id) {
                entries.retain(|e| e.key != key);
            }
            Response {
                id: request_id,
                result: Some(serde_json::json!({ "status": "knowledge_deleted" })),
                error: None,
            }
        }
        BrowserCommand::GetSessionHistory => {
            let history = state
                .sessions
                .get(session_id)
                .map(|s| s.history.clone())
                .unwrap_or_default();
            Response {
                id: request_id,
                result: Some(serde_json::json!({ "history": history })),
                error: None,
            }
        }
        BrowserCommand::GetCapabilities => {
            let capabilities = state
                .sessions
                .get(session_id)
                .and_then(|s| s.current_binary.as_ref().map(|b| b.capabilities.clone()))
                .unwrap_or_default();
            Response {
                id: request_id,
                result: Some(serde_json::json!({ "capabilities": capabilities })),
                error: None,
            }
        }
        BrowserCommand::SetAutonomousMode { enabled } => {
            if let Some(mut session) = state.sessions.get_mut(session_id) {
                session.autonomous_mode = enabled;
                Response {
                    id: request_id,
                    result: Some(
                        serde_json::json!({ "status": "autonomous_mode_set", "enabled": enabled }),
                    ),
                    error: None,
                }
            } else {
                Response {
                    id: request_id,
                    result: None,
                    error: Some("Session not found".to_string()),
                }
            }
        }
        BrowserCommand::SwarmSearch { query, depth } => {
            info!(
                "Initiating swarm search for '{}' with depth {}",
                query, depth
            );
            let cluster = state.cluster.lock().await;
            let _ = cluster
                .broadcast_swarm_search(query.clone(), depth, request_id.clone())
                .await;
            Response {
                id: request_id,
                result: Some(
                    serde_json::json!({ "status": "swarm_search_initiated", "query": query, "depth": depth }),
                ),
                error: None,
            }
        }
        BrowserCommand::DelegateTask {
            task,
            target_agent_id,
        } => {
            info!("Delegating task: '{}' to {:?}", task, target_agent_id);
            let cluster = state.cluster.lock().await;
            let _ = cluster.delegate_task(task.clone(), target_agent_id).await;
            Response {
                id: request_id,
                result: Some(serde_json::json!({ "status": "task_delegated", "task": task })),
                error: None,
            }
        }
        BrowserCommand::ProposeKnowledge { key, value, tags } => {
            info!("Proposing knowledge: {} = {:?}", key, value);
            let cluster = state.cluster.lock().await;
            let _ = cluster
                .propose_knowledge(key.clone(), value.clone(), tags.clone())
                .await;
            Response {
                id: request_id,
                result: Some(serde_json::json!({ "status": "knowledge_proposed", "key": key })),
                error: None,
            }
        }
        BrowserCommand::CreateSwarmPlan { goal } => {
            info!("Creating swarm plan for goal: '{}'", goal);
            let plan_id = Uuid::new_v4();

            // Simulate plan generation (in a real app, this would use an LLM)
            let tasks = vec![
                spine_protocol::PlanTask {
                    id: Uuid::new_v4(),
                    description: format!("Research: {}", goal),
                    required_skills: vec!["research".to_string(), "scraping".to_string()],
                    assigned_to: None,
                    dependencies: vec![],
                    status: spine_protocol::TaskStatus::Pending,
                    result: None,
                },
                spine_protocol::PlanTask {
                    id: Uuid::new_v4(),
                    description: format!("Synthesize findings for: {}", goal),
                    required_skills: vec!["synthesis".to_string()],
                    assigned_to: None,
                    dependencies: vec![], // Would depend on the first task
                    status: spine_protocol::TaskStatus::Pending,
                    result: None,
                },
            ];

            let plan = spine_protocol::SwarmPlan {
                id: plan_id,
                goal: goal.clone(),
                tasks,
                status: spine_protocol::PlanStatus::Active,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };

            state.plans.insert(plan_id, plan.clone());

            let cluster = state.cluster.lock().await;
            let _ = cluster.propose_swarm_plan(plan).await;

            Response {
                id: request_id,
                result: Some(serde_json::json!({ "status": "plan_created", "plan_id": plan_id })),
                error: None,
            }
        }
        BrowserCommand::ExecutePlanTask { plan_id, task_id } => {
            info!("Executing plan task {} for plan {}", task_id, plan_id);

            if let Some(mut plan) = state.plans.get_mut(&plan_id) {
                if let Some(task) = plan.tasks.iter_mut().find(|t| t.id == task_id) {
                    task.status = spine_protocol::TaskStatus::InProgress;

                    // Broadcast update
                    let cluster = state.cluster.lock().await;
                    let _ = cluster
                        .update_plan_task(plan_id, task_id, task.status.clone(), None)
                        .await;

                    Response {
                        id: request_id,
                        result: Some(
                            serde_json::json!({ "status": "task_started", "task_id": task_id }),
                        ),
                        error: None,
                    }
                } else {
                    Response {
                        id: request_id,
                        result: None,
                        error: Some("Task not found".to_string()),
                    }
                }
            } else {
                Response {
                    id: request_id,
                    result: None,
                    error: Some("Plan not found".to_string()),
                }
            }
        }
        BrowserCommand::NeuralTransmit { data, domain } => {
            let domain_enum = match domain.as_str() {
                "RealTime" => spine_agentic::ProtocolDomain::RealTime,
                "BulkData" => spine_agentic::ProtocolDomain::BulkData,
                "SecureControl" => spine_agentic::ProtocolDomain::SecureControl,
                "IoT" => spine_agentic::ProtocolDomain::IoT,
                _ => spine_agentic::ProtocolDomain::BulkData,
            };

            let mut protocol = state
                .neural_protocols
                .entry(domain.clone())
                .or_insert_with(|| spine_agentic::NeuralProtocol::new(1000.0, 5.0));
            match protocol.transmit(&data, domain_enum).await {
                Ok(stats) => Response {
                    id: request_id,
                    result: Some(serde_json::to_value(stats).unwrap_or_default()),
                    error: None,
                },
                Err(e) => Response {
                    id: request_id,
                    result: None,
                    error: Some(e),
                },
            }
        }
        BrowserCommand::GetAgenticState => {
            let runtime = state.agentic_runtime.clone();
            let profile = runtime.profile();
            let variant = profile.miras_variant.clone();
            let surprise = 0.15;

            Response {
                id: request_id,
                result: Some(serde_json::json!({
                    "miras_variant": variant,
                    "surprise_level": surprise,
                    "agent_id": profile.id,
                    "trust_level": format!("{:?}", profile.trust_level),
                })),
                error: None,
            }
        }
        BrowserCommand::SendSpeechAct {
            target_id,
            performative,
            content,
        } => {
            info!(
                "Sending speech act to {}: {} - {}",
                target_id, performative, content
            );
            Response {
                id: request_id,
                result: Some(serde_json::json!({ "status": "sent" })),
                error: None,
            }
        }
    };

    timer.observe_duration();
    (response, latent_to_stream)
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = SpineConfig::load();
    init_telemetry("spine-core")?;
    info!("Starting SPINE Agentic Browser...");
    info!(
        "Config: host={}, port={}, max_sessions={}, tls={}",
        config.server.host, config.server.port, config.server.max_sessions, config.tls.enabled
    );

    let port = config.server.port;
    let cluster_port = port + config.cluster.port_offset;
    let cluster_addr: std::net::SocketAddr = format!("{}:{}", config.server.host, cluster_port)
        .parse()
        .context("invalid cluster address")?;

    let capabilities = NodeCapabilities {
        supports_wasm: true,
        supports_chameleon: true,
        supports_speculation: true,
        max_sessions: config.server.max_sessions,
        region: Some(config.cluster.region.clone()),
        skills: config.cluster.skills.clone(),
    };

    let mut cluster_node = ClusterNode::new(cluster_addr, capabilities);
    cluster_node.start().await?;

    let profile = spine_agentic::AgentProfile::new("SPINE Core Agent")
        .with_capabilities(vec![
            spine_agentic::AgentCapability::Navigation,
            spine_agentic::AgentCapability::ContentExtraction,
            spine_agentic::AgentCapability::AgentCommunication,
            spine_agentic::AgentCapability::KnowledgeManagement,
            spine_agentic::AgentCapability::SwarmParticipation,
        ])
        .with_trust(spine_agentic::TrustLevel::Core);
    let agentic_runtime = Arc::new(spine_agentic::AgenticWebRuntime::new(profile));

    let state = Arc::new(BrowserState {
        sessions: DashMap::new(),
        knowledge_base: DashMap::new(),
        proposals: DashMap::new(),
        plans: DashMap::new(),
        client: reqwest::Client::new(),
        encoder: tokio::sync::RwLock::new(NeuralLatentEncoder::new(256, 1024, &[512, 256], 8, 42)),
        cluster: Mutex::new(cluster_node),
        agentic_runtime,
        rate_limits: DashMap::new(),
        wasm_runtime: WasmRuntime::new().expect("Failed to initialize WASM runtime"),
        neural_protocols: DashMap::new(),
        connections_per_ip: DashMap::new(),
        active_connections: std::sync::atomic::AtomicUsize::new(0),
        start_time: std::time::Instant::now(),
        shutting_down: std::sync::atomic::AtomicBool::new(false),
        config: config.clone(),
    });

    // Load persisted sessions
    if let Err(e) = state.load_sessions() {
        warn!("Failed to load persisted sessions: {}", e);
    } else {
        info!("Loaded {} persisted sessions", state.sessions.len());
    }

    // Start session persistence task
    let persistence_state = state.clone();
    let persist_secs = config.server.persistence_interval_secs;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(persist_secs));
        loop {
            interval.tick().await;
            if let Err(e) = persistence_state.save_sessions().await {
                error!("Failed to save sessions: {}", e);
            } else {
                debug!("Sessions persisted to disk");
            }
        }
    });

    // Start metrics & health server
    let metrics_state = state.clone();
    let health_state = state.clone();
    let ready_state = state.clone();
    let metrics_app = Router::new()
        .route("/metrics", get(|| async {
            let encoder = TextEncoder::new();
            let metric_families = prometheus::gather();
            let mut buffer = vec![];
            let _ = encoder.encode(&metric_families, &mut buffer);
            String::from_utf8(buffer).unwrap_or_default()
        }))
        .route("/health", get(move || {
            let state = health_state.clone();
            async move {
                let uptime = state.start_time.elapsed().as_secs();
                let active = state.active_connections.load(std::sync::atomic::Ordering::Relaxed);
                let sessions = state.sessions.len();
                let shutting_down = state.shutting_down.load(std::sync::atomic::Ordering::Relaxed);

                let status = if shutting_down { "draining" } else { "healthy" };
                let code = if shutting_down { 503u16 } else { 200 };

                let body = serde_json::json!({
                    "status": status,
                    "uptime_secs": uptime,
                    "active_connections": active,
                    "sessions": sessions,
                    "version": env!("CARGO_PKG_VERSION"),
                });

                (
                    axum::http::StatusCode::from_u16(code).unwrap_or(axum::http::StatusCode::OK),
                    axum::response::Json(body),
                )
            }
        }))
        .route("/ready", get(move || {
            let state = ready_state.clone();
            async move {
                let shutting_down = state.shutting_down.load(std::sync::atomic::Ordering::Relaxed);
                let sessions = state.sessions.len();
                let max = state.config.server.max_sessions;
                let ready = !shutting_down && sessions < max;

                let body = serde_json::json!({
                    "ready": ready,
                    "sessions": sessions,
                    "max_sessions": max,
                });

                let code = if ready { 200u16 } else { 503 };
                (
                    axum::http::StatusCode::from_u16(code).unwrap_or(axum::http::StatusCode::OK),
                    axum::response::Json(body),
                )
            }
        }))
        .route("/dashboard", get(move || {
            let state = metrics_state.clone();
            async move {
                let mut html = String::from("<html><head><title>SPINE Dashboard</title>");
                html.push_str("<style>body{font-family:sans-serif;padding:20px;} table{border-collapse:collapse;width:100%;} th,td{border:1px solid #ddd;padding:8px;text-align:left;} th{background-color:#f2f2f2;}</style>");
                html.push_str("</head><body><h1>SPINE Session Monitor</h1>");
                html.push_str(&format!("<p>Active Sessions: {}</p>", state.sessions.len()));
                html.push_str("<table><tr><th>Session ID</th><th>URL</th><th>Last Command</th></tr>");
                for entry in state.sessions.iter() {
                    let (id, session) = entry.pair();
                    html.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{:?}</td></tr>",
                        id,
                        session.current_url.as_deref().unwrap_or("None"),
                        session.last_command));
                }
                html.push_str("</table></body></html>");
                axum::response::Html(html)
            }
        }));

    let metrics_port = config.server.metrics_port;
    tokio::spawn(async move {
        let addr: std::net::SocketAddr = match format!("0.0.0.0:{}", metrics_port).parse() {
            Ok(a) => a,
            Err(e) => {
                error!("Invalid metrics address: {}", e);
                return;
            }
        };
        info!("Health & metrics server on http://{}", addr);
        if let Err(e) = axum::Server::bind(&addr)
            .serve(metrics_app.into_make_service())
            .await
        {
            error!("Metrics server error: {}", e);
        }
    });

    // Start cluster event listener
    let cluster_state = state.clone();
    let event_rx = {
        let cluster = state.cluster.lock().await;
        cluster.get_event_receiver()
    };

    tokio::spawn(async move {
        let mut rx = event_rx.lock().await;
        while let Some(event) = rx.recv().await {
            match event {
                spine_cluster::ClusterEvent::SessionTransferRequested {
                    session_id,
                    from_node,
                } => {
                    info!(
                        "Session transfer requested for {} from node {}",
                        session_id, from_node
                    );
                    // In a real implementation, we might need to acknowledge or prepare
                }
                spine_cluster::ClusterEvent::SessionDataReceived { session_id, data } => {
                    info!("Received session data for {}", session_id);
                    if let Ok(session) = serde_json::from_slice::<Session>(&data) {
                        cluster_state.sessions.insert(session_id, session);
                    }
                }
                spine_cluster::ClusterEvent::SearchRequested {
                    query,
                    request_id,
                    origin_node,
                } => {
                    info!(
                        "Distributed search requested: '{}' from node {}",
                        query, origin_node
                    );

                    let mut results = Vec::new();
                    for session in cluster_state.sessions.iter() {
                        if let Some(html) = &session.current_html {
                            if html.to_lowercase().contains(&query.to_lowercase()) {
                                results.push(serde_json::json!({
                                    "session_id": session.key(),
                                    "url": session.current_url,
                                    "relevance": 0.95,
                                    "snippet": format!("Found match for '{}' in active session", query)
                                }));
                            }
                        }
                    }

                    let results_json = serde_json::Value::Array(results);
                    let cluster = cluster_state.cluster.lock().await;
                    let _ = cluster.send_search_results(origin_node, request_id, results_json);
                }
                spine_cluster::ClusterEvent::SearchResultReceived {
                    request_id,
                    results: _,
                    node_id,
                } => {
                    info!(
                        "Received search results for {} from node {}",
                        request_id, node_id
                    );
                    // In a real app, we'd push this to the client via a WebSocket or similar
                }
                spine_cluster::ClusterEvent::KnowledgeSynced {
                    key,
                    value,
                    tags,
                    origin_node,
                } => {
                    info!(
                        "Knowledge synced from node {}: {} = {}",
                        origin_node, key, value
                    );
                    // Store in a global/shared knowledge base or specific session if applicable
                    // For now, we'll store it in a "cluster_shared" session
                    let entry = KnowledgeEntry {
                        key,
                        value,
                        tags,
                        timestamp_ns: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64,
                    };
                    cluster_state
                        .knowledge_base
                        .entry("cluster_shared".to_string())
                        .or_default()
                        .push(entry);
                }
                spine_cluster::ClusterEvent::SwarmSearchRequested {
                    query,
                    depth,
                    request_id: _,
                    origin_node,
                } => {
                    info!(
                        "Swarm search requested: '{}' (depth {}) from node {}",
                        query, depth, origin_node
                    );
                    // Spawn a "Scout" session to perform the search
                    let scout_id = format!("scout-{}-{}", origin_node, Uuid::new_v4());
                    let mut scout_session = Session::new();
                    scout_session.current_url =
                        Some(format!("https://www.google.com/search?q={}", query));
                    scout_session.autonomous_mode = true;
                    cluster_state
                        .sessions
                        .insert(scout_id.clone(), scout_session);

                    // In a real implementation, we'd wait for the scout to finish and send results back
                }
                spine_cluster::ClusterEvent::TaskDelegated {
                    task,
                    target_agent_id,
                    origin_node,
                } => {
                    info!(
                        "Task delegated from node {}: '{}' (target: {:?})",
                        origin_node, task, target_agent_id
                    );
                    // Handle task delegation (e.g., assign to an idle autonomous session)
                }
                spine_cluster::ClusterEvent::KnowledgeProposed {
                    proposal_id,
                    key,
                    value,
                    tags,
                    origin_node,
                } => {
                    info!(
                        "Knowledge proposal received from node {}: {} = {:?}",
                        origin_node, key, value
                    );

                    // Store proposal
                    cluster_state.proposals.insert(
                        proposal_id,
                        KnowledgeProposal {
                            key: key.clone(),
                            value: value.clone(),
                            tags: tags.clone(),
                            votes: Vec::new(),
                            origin_node,
                        },
                    );

                    // Auto-vote based on confidence (simulated)
                    let cluster = cluster_state.cluster.lock().await;
                    let _ = cluster.vote_on_knowledge(proposal_id, true, 0.9).await;
                }
                spine_cluster::ClusterEvent::KnowledgeVoteReceived {
                    proposal_id,
                    voter_id,
                    approved,
                    confidence,
                } => {
                    info!(
                        "Vote received for proposal {} from node {}: approved={}, confidence={:.2}",
                        proposal_id, voter_id, approved, confidence
                    );

                    if let Some(mut proposal) = cluster_state.proposals.get_mut(&proposal_id) {
                        proposal.votes.push(KnowledgeVote {
                            voter_id,
                            approved,
                            confidence,
                        });

                        // Check if consensus reached using weighted voting
                        let cluster = cluster_state.cluster.lock().await;
                        let total_nodes = cluster.get_healthy_nodes().len() + 1; // +1 for self

                        // Compute weighted approval score using voter confidence
                        let weighted_approval: f32 = proposal
                            .votes
                            .iter()
                            .filter(|v| v.approved)
                            .map(|v| v.confidence)
                            .sum();
                        let total_confidence: f32 =
                            proposal.votes.iter().map(|v| v.confidence).sum();
                        let weighted_ratio = if total_confidence > 0.0 {
                            weighted_approval / total_confidence
                        } else {
                            0.0
                        };

                        // Log vote breakdown by voter
                        for vote in proposal.votes.iter() {
                            debug!(
                                "  Voter {}: approved={}, confidence={:.2}",
                                vote.voter_id, vote.approved, vote.confidence
                            );
                        }

                        let approved_votes = proposal.votes.iter().filter(|v| v.approved).count();
                        let threshold = cluster.get_consensus_threshold();
                        info!(
                            "Proposal {} from node {}: {}/{} votes ({:.1}% weighted approval)",
                            proposal_id,
                            proposal.origin_node,
                            approved_votes,
                            total_nodes,
                            weighted_ratio * 100.0
                        );

                        if (approved_votes as f32 / total_nodes as f32) >= threshold {
                            info!(
                                "Consensus reached for proposal {}. Committing...",
                                proposal_id
                            );
                            let _ = cluster.commit_knowledge(
                                proposal_id,
                                proposal.key.clone(),
                                proposal.value.clone(),
                                proposal.tags.clone(),
                            );
                        }
                    }
                }
                spine_cluster::ClusterEvent::KnowledgeCommitted {
                    proposal_id,
                    key,
                    value,
                    tags,
                } => {
                    info!(
                        "Knowledge proposal {} committed: {} = {:?}",
                        proposal_id, key, value
                    );

                    let entry = KnowledgeEntry {
                        key,
                        value,
                        tags,
                        timestamp_ns: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64,
                    };
                    cluster_state
                        .knowledge_base
                        .entry("cluster_consensus".to_string())
                        .or_default()
                        .push(entry);
                    cluster_state.proposals.remove(&proposal_id);
                }
                spine_cluster::ClusterEvent::SwarmPlanProposed { plan, origin_node } => {
                    info!(
                        "Swarm plan proposed by node {}: '{}' ({} tasks)",
                        origin_node,
                        plan.goal,
                        plan.tasks.len()
                    );
                    cluster_state.plans.insert(plan.id, plan);
                }
                spine_cluster::ClusterEvent::PlanTaskUpdated {
                    plan_id,
                    task_id,
                    status,
                    result,
                    node_id,
                } => {
                    info!(
                        "Plan task {} updated by node {}: status={:?}",
                        task_id, node_id, status
                    );
                    if let Some(mut plan) = cluster_state.plans.get_mut(&plan_id) {
                        if let Some(task) = plan.tasks.iter_mut().find(|t| t.id == task_id) {
                            task.status = status;
                            task.result = result;
                        }
                    }
                }
                _ => {}
            }
        }
    });

    // Start autonomous agent loop
    let agent_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;

            // 1. Swarm Task Scheduling
            for mut plan_entry in agent_state.plans.iter_mut() {
                let plan_id = *plan_entry.key();
                let plan = plan_entry.value_mut();

                if plan.status == spine_protocol::PlanStatus::Active {
                    let completed_tasks: Vec<_> = plan
                        .tasks
                        .iter()
                        .filter(|t| t.status == spine_protocol::TaskStatus::Completed)
                        .map(|t| t.id)
                        .collect();

                    for task in plan.tasks.iter_mut() {
                        if task.status == spine_protocol::TaskStatus::Pending
                            && task.assigned_to.is_none()
                        {
                            // Check dependencies
                            let deps_met = task
                                .dependencies
                                .iter()
                                .all(|dep_id| completed_tasks.contains(dep_id));

                            if !deps_met {
                                continue;
                            }

                            // Skill-based Routing
                            let cluster = agent_state.cluster.lock().await;
                            let mut best_node = None;

                            // Check self first
                            let my_id = cluster.id;
                            let my_caps = cluster.get_capabilities();
                            let my_score = task
                                .required_skills
                                .iter()
                                .filter(|s| my_caps.skills.contains(s))
                                .count();

                            if my_score > 0 {
                                best_node = Some((my_id, my_score));
                            }

                            // Check other nodes
                            for node in cluster.get_healthy_nodes() {
                                if node.id == my_id {
                                    continue;
                                }
                                let score = task
                                    .required_skills
                                    .iter()
                                    .filter(|s| node.capabilities.skills.contains(s))
                                    .count();

                                if score > best_node.map(|(_, s)| s).unwrap_or(0) {
                                    best_node = Some((node.id, score));
                                }
                            }

                            if let Some((node_id, _)) = best_node {
                                info!("Swarm Scheduler: Assigning task '{}' to node {} (skills matched)", task.description, node_id);
                                task.assigned_to = Some(node_id);

                                if node_id == my_id {
                                    task.status = spine_protocol::TaskStatus::InProgress;
                                    // Broadcast assignment
                                    let _ = cluster
                                        .update_plan_task(
                                            plan_id,
                                            task.id,
                                            task.status.clone(),
                                            None,
                                        )
                                        .await;

                                    // Trigger task execution (simulated)
                                    let task_desc = task.description.clone();
                                    let task_id = task.id;
                                    let task_state = agent_state.clone();
                                    tokio::spawn(async move {
                                        tokio::time::sleep(std::time::Duration::from_secs(10))
                                            .await;
                                        info!("Task completed: {}", task_desc);
                                        if let Some(mut p) = task_state.plans.get_mut(&plan_id) {
                                            if let Some(t) =
                                                p.tasks.iter_mut().find(|t| t.id == task_id)
                                            {
                                                t.status = spine_protocol::TaskStatus::Completed;
                                                t.result = Some(
                                                    serde_json::json!({ "status": "success", "data": "Simulated result" }),
                                                );

                                                let cluster = task_state.cluster.lock().await;
                                                let _ = cluster
                                                    .update_plan_task(
                                                        plan_id,
                                                        task_id,
                                                        t.status.clone(),
                                                        t.result.clone(),
                                                    )
                                                    .await;
                                            }
                                        }
                                    });
                                } else {
                                    // Task assigned to another node, they will pick it up in their loop
                                    let _ = cluster
                                        .update_plan_task(
                                            plan_id,
                                            task.id,
                                            task.status.clone(),
                                            None,
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                }
            }

            // 2. Autonomous Session Loop
            for mut entry in agent_state.sessions.iter_mut() {
                let session_id = entry.key().clone();
                let session = entry.value_mut();

                if session.autonomous_mode {
                    if let Some(html) = &session.current_html {
                        match spine_parser::parse_html(html) {
                            Ok(ur) => {
                                let engine = spine_human::ReasoningEngine::new();
                                let plan = engine.create_plan("Explore and find search", &ur);

                                if let Some(best_action) = plan.steps.first() {
                                    if plan.estimated_success > 0.7 {
                                        info!("Autonomous agent in session {} executing plan step: {}", session_id, best_action.action_type);

                                        // Execute the action
                                        match best_action.action_type.as_str() {
                                            "Search" | "Authenticate" => {
                                                if let Some(target_id) = &best_action.target_id {
                                                    session.history.push(
                                                        spine_protocol::BrowserCommand::Click {
                                                            element_id: target_id.clone(),
                                                        },
                                                    );
                                                }
                                            }
                                            "InputSearch" => {
                                                if let Some(target_id) = &best_action.target_id {
                                                    session.history.push(
                                                        spine_protocol::BrowserCommand::Type {
                                                            element_id: target_id.clone(),
                                                            text: "SPINE Browser".to_string(),
                                                        },
                                                    );
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            Err(e) => warn!(
                                "Failed to parse HTML for autonomous session {}: {}",
                                session_id, e
                            ),
                        }
                    }
                }
            }
        }
    });

    // Start autonomous security monitor
    let monitor_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            info!("Autonomous security monitor: checking session health...");
            for mut session in monitor_state.sessions.iter_mut() {
                // If a session has been active but hasn't morphed recently, trigger a morph
                // This is a simple "agentic" self-healing behavior
                if session.last_command.is_some() && !session.needs_morph {
                    info!(
                        "Self-healing: triggering proactive protocol morph for session {}",
                        session.key()
                    );
                    session.needs_morph = true;
                }
            }
        }
    });

    let addr = format!("{}:{}", config.server.host, port);
    let listener = TcpListener::bind(&addr).await?;
    info!("TCP listening on {}", addr);

    // WebSocket listener on port + offset
    let ws_port = port + config.server.ws_port_offset;
    let ws_addr = format!("{}:{}", config.server.host, ws_port);
    let ws_listener = TcpListener::bind(&ws_addr).await?;
    info!("WebSocket listening on ws://{}", ws_addr);

    // QUIC listener on port + offset
    #[cfg(feature = "quic")]
    let quic_port = port + config.server.quic_port_offset;
    #[cfg(feature = "quic")]
    {
        let quic_addr: std::net::SocketAddr = format!("{}:{}", config.server.host, quic_port)
            .parse()
            .context("invalid QUIC address")?;
        let quic_builder = spine_protocol::QuicEndpointBuilder::new();
        let quic_endpoint = quic_builder.build_server(quic_addr)?;
        info!("QUIC listening on {}", quic_addr);

        let quic_state = Arc::clone(&state);
        tokio::spawn(async move {
            while let Some(incoming) = quic_endpoint.accept().await {
                let quic_state = Arc::clone(&quic_state);
                tokio::spawn(async move {
                    match incoming.await {
                        Ok(connection) => {
                            info!("New QUIC connection from {}", connection.remote_address());
                            match connection.accept_bi().await {
                                Ok((send, recv)) => {
                                    let stream = spine_transport::QuicStream::new(send, recv);
                                    let mut handler = ProtocolHandler::new(stream);
                                    handle_session(&mut handler, quic_state).await;
                                }
                                Err(e) => error!("QUIC stream accept failed: {}", e),
                            }
                        }
                        Err(e) => error!("QUIC connection failed: {}", e),
                    }
                });
            }
        });
    }

    // TLS setup (uses config with env-var override)
    let tls_acceptor = if config.tls.enabled {
        let cert_path = std::path::Path::new(&config.tls.cert_path);
        let key_path = std::path::Path::new(&config.tls.key_path);
        let ca_path = std::path::Path::new(&config.tls.ca_path);
        let ca_opt = if ca_path.exists() {
            Some(ca_path)
        } else {
            None
        };

        if cert_path.exists() && key_path.exists() {
            Some(create_tls_acceptor(cert_path, key_path, ca_opt)?)
        } else {
            info!(
                "TLS enabled but certs not found at {}. Falling back to plain TCP.",
                config.tls.cert_path
            );
            None
        }
    } else {
        None
    };

    // Watchdog: periodically reap idle sessions
    let watchdog_state = Arc::clone(&state);
    let watchdog_secs = config.server.session_watchdog_secs;
    let idle_timeout = std::time::Duration::from_secs(config.server.idle_timeout_secs);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(watchdog_secs));
        loop {
            interval.tick().await;
            let now = std::time::Instant::now();
            let mut reaped = 0u32;
            let ids_to_remove: Vec<String> = watchdog_state
                .sessions
                .iter()
                .filter_map(|entry| {
                    let session = entry.value();
                    // Sessions with no recent command and past idle timeout
                    if session.last_command.is_some() {
                        // We don't have per-session timestamps yet, use a heuristic:
                        // sessions in autonomous mode are kept alive
                        if session.autonomous_mode {
                            return None;
                        }
                    }
                    // For now, reap sessions that have no URL and no commands (abandoned)
                    if session.current_url.is_none()
                        && session.last_command.is_none()
                        && session.history.is_empty()
                    {
                        Some(entry.key().clone())
                    } else {
                        None
                    }
                })
                .collect();

            for id in ids_to_remove {
                watchdog_state.sessions.remove(&id);
                SESSIONS_ACTIVE.dec();
                reaped += 1;
            }
            if reaped > 0 {
                info!(
                    "Watchdog: reaped {} idle sessions, {} remaining",
                    reaped,
                    watchdog_state.sessions.len()
                );
            }
            let _ = now; // suppress unused warning for future per-session timestamps
            let _ = idle_timeout;
        }
    });

    // Graceful shutdown signal
    let shutdown_state = Arc::clone(&state);
    let shutdown_timeout = std::time::Duration::from_secs(config.server.shutdown_timeout_secs);
    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    let shutdown_notify_signal = Arc::clone(&shutdown_notify);
    tokio::spawn(async move {
        // Wait for Ctrl+C
        if let Err(e) = tokio::signal::ctrl_c().await {
            error!("Failed to listen for ctrl_c: {}", e);
            return;
        }
        info!("Shutdown signal received — draining connections...");
        shutdown_state
            .shutting_down
            .store(true, std::sync::atomic::Ordering::SeqCst);

        // Save sessions before shutdown
        if let Err(e) = shutdown_state.save_sessions().await {
            error!("Failed to save sessions during shutdown: {}", e);
        } else {
            info!("Sessions persisted successfully");
        }

        // Wait for active connections to drain (up to timeout)
        let deadline = tokio::time::Instant::now() + shutdown_timeout;
        loop {
            let active = shutdown_state
                .active_connections
                .load(std::sync::atomic::Ordering::Relaxed);
            if active == 0 {
                info!("All connections drained");
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                warn!(
                    "Shutdown timeout — {} connections still active, forcing exit",
                    active
                );
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }

        info!("SPINE server shut down");
        shutdown_notify_signal.notify_one();
    });

    let max_per_ip = config.server.max_connections_per_ip;

    loop {
        tokio::select! {
            // Shutdown gate
            _ = shutdown_notify.notified() => {
                info!("Accept loop terminated by shutdown signal");
                break;
            }

            // Accept TCP connections
            result = listener.accept() => {
                let (socket, addr) = result?;

                // Enforce per-IP connection limit
                let ip = addr.ip();
                let current = {
                    let entry = state.connections_per_ip.entry(ip).or_insert_with(|| std::sync::atomic::AtomicUsize::new(0));
                    entry.value().load(std::sync::atomic::Ordering::Relaxed)
                };
                if current >= max_per_ip {
                    warn!("Per-IP limit reached for {} ({}/{}), rejecting", ip, current, max_per_ip);
                    drop(socket);
                    continue;
                }

                // Check shutdown flag
                if state.shutting_down.load(std::sync::atomic::Ordering::Relaxed) {
                    info!("Rejecting connection during shutdown");
                    drop(socket);
                    continue;
                }

                let span = span!(Level::INFO, "connection", remote_addr = %addr, transport = "tcp");
                let _enter = span.enter();
                info!("New TCP connection from {}", addr);

                // Track connection
                state.active_connections.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                state.connections_per_ip.entry(ip).or_insert_with(|| std::sync::atomic::AtomicUsize::new(0))
                    .value().fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                let conn_state = Arc::clone(&state);
                let tls_acceptor = tls_acceptor.clone();

                tokio::spawn(async move {
                    if let Some(acceptor) = tls_acceptor {
                        match acceptor.accept(socket).await {
                            Ok(tls_stream) => {
                                let mut handler = ProtocolHandler::new(tls_stream);
                                handle_session(&mut handler, conn_state.clone()).await;
                            }
                            Err(e) => error!("TLS handshake failed: {}", e),
                        }
                    } else {
                        let mut handler = ProtocolHandler::new(socket);
                        handle_session(&mut handler, conn_state.clone()).await;
                    }
                    // Decrement counters on disconnect
                    conn_state.active_connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    conn_state.connections_per_ip.entry(ip).and_modify(|v| { v.fetch_sub(1, std::sync::atomic::Ordering::Relaxed); });
                });
            }

            // Accept WebSocket connections
            result = ws_listener.accept() => {
                let (socket, addr) = result?;

                let ip = addr.ip();
                let current = {
                    let entry = state.connections_per_ip.entry(ip).or_insert_with(|| std::sync::atomic::AtomicUsize::new(0));
                    entry.value().load(std::sync::atomic::Ordering::Relaxed)
                };
                if current >= max_per_ip {
                    warn!("Per-IP limit reached for {} (WS), rejecting", ip);
                    drop(socket);
                    continue;
                }
                if state.shutting_down.load(std::sync::atomic::Ordering::Relaxed) {
                    drop(socket);
                    continue;
                }

                info!("New WebSocket connection from {}", addr);

                state.active_connections.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                state.connections_per_ip.entry(ip).or_insert_with(|| std::sync::atomic::AtomicUsize::new(0))
                    .value().fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                let conn_state = Arc::clone(&state);
                tokio::spawn(async move {
                    match spine_transport::WebSocketServerBridge::accept(socket).await {
                        Ok(bridge) => {
                            let stream = WebSocketStream::new(bridge);
                            let mut handler = ProtocolHandler::new(stream);
                            handle_session(&mut handler, conn_state.clone()).await;
                        }
                        Err(e) => error!("WebSocket upgrade failed: {}", e),
                    }
                    conn_state.active_connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    conn_state.connections_per_ip.entry(ip).and_modify(|v| { v.fetch_sub(1, std::sync::atomic::Ordering::Relaxed); });
                });
            }
        }
    }

    // Final session save on clean shutdown
    if let Err(e) = state.save_sessions().await {
        error!("Failed to save sessions on exit: {}", e);
    }
    info!("Server exited cleanly");
    Ok(())
}

async fn handle_session<S>(handler: &mut ProtocolHandler<S>, state: Arc<BrowserState>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut session_id: Option<String> = None;

    loop {
        match handler.receive_message().await {
            Ok(Message::Ping { timestamp }) => {
                let _ = handler.send_message(&Message::Pong { timestamp }).await;
            }
            Ok(Message::Request(req)) => {
                let req_span =
                    span!(Level::INFO, "request", request_id = %req.id, command = ?req.command);
                let _enter = req_span.enter();
                info!("Received request: {:?}", req);

                // 1. Check session limit for new sessions
                if session_id.is_none() && state.sessions.len() >= state.config.server.max_sessions
                {
                    let _ = handler
                        .send_message(&Message::Response(Response {
                            id: req.id,
                            result: None,
                            error: Some("Server busy: maximum sessions reached".to_string()),
                        }))
                        .await;
                    return;
                }

                // 2. Ensure session exists
                let id = if let Some(id) = session_id.as_ref() {
                    id.clone()
                } else {
                    let new_id = Uuid::new_v4().to_string();
                    SESSIONS_ACTIVE.inc();
                    state.sessions.insert(new_id.clone(), Session::new());

                    // Register session with cluster
                    let cluster = state.cluster.lock().await;
                    cluster.register_local_session(new_id.clone());

                    session_id = Some(new_id.clone());
                    new_id
                };

                // 3. Enforce rate limit
                if !state.check_rate_limit(&id) {
                    let _ = handler
                        .send_message(&Message::Response(Response {
                            id: req.id,
                            result: None,
                            error: Some("Rate limit exceeded".to_string()),
                        }))
                        .await;
                    continue;
                }

                // Check if session needs a proactive morph
                if let Some(mut session) = state.sessions.get_mut(&id) {
                    if session.needs_morph {
                        let seed = rand::random::<u64>();
                        info!("Sending proactive MorphRequest for session {}", id);
                        if handler
                            .send_message(&Message::MorphRequest { seed })
                            .await
                            .is_ok()
                        {
                            handler.morph_now(seed);
                            session.needs_morph = false;
                        }
                    }
                }

                // Handle the command
                let (res, latent_to_stream) = if let BrowserCommand::Morph = req.command {
                    let seed = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as u64;
                    handler.morph_now(seed);
                    PROTOCOL_MORPHS.inc();
                    (
                        Response {
                            id: req.id,
                            result: Some(serde_json::json!({ "status": "morphed", "seed": seed })),
                            error: None,
                        },
                        Vec::new(),
                    )
                } else {
                    let (res, latent) =
                        handle_command(&state, &id, req.command.clone(), req.id).await;
                    if let Some(mut session) = state.sessions.get_mut(&id) {
                        session.last_command = Some(req.command);
                    }
                    (res, latent)
                };

                if let Err(e) = handler.send_message(&Message::Response(res)).await {
                    error!("Failed to send response: {}", e);
                }

                // Stream any latent vectors produced
                for vector in latent_to_stream {
                    let latent_msg = spine_protocol::LatentVector {
                        components: vector,
                        dim_hint: 0,
                        epoch: 0,
                    };
                    if let Err(e) = handler
                        .send_message(&Message::LatentMessage(latent_msg))
                        .await
                    {
                        error!("Failed to stream latent vector: {}", e);
                    }
                }

                // Speculative pre-computation for likely next requests
                let state_clone = state.clone();
                let id_clone = id.clone();
                handler.speculate_responses(|predicted_hash| {
                    let session = state_clone.sessions.get(&id_clone)?;
                    let last_cmd = session.last_command.as_ref()?;

                    match last_cmd {
                        BrowserCommand::Navigate { .. } => {
                            // If we just navigated, the agent will likely ask for UR
                            if let Some(html) = &session.current_html {
                                if let Ok(ur) = parse_html(html) {
                                    return Some(Message::PreComputed(PreComputedResponse {
                                        request_hash: predicted_hash,
                                        result: serde_json::to_value(ur).unwrap_or_default(),
                                        confidence: 0.85,
                                        alternatives: Vec::new(),
                                    }));
                                }
                            }
                        }
                        BrowserCommand::GetUR => {
                            // If we just got UR, the agent might want raw HTML for transpilation
                            if let Some(html) = &session.current_html {
                                return Some(Message::PreComputed(PreComputedResponse {
                                    request_hash: predicted_hash,
                                    result: serde_json::json!({ "html": html }),
                                    confidence: 0.7,
                                    alternatives: Vec::new(),
                                }));
                            }
                        }
                        _ => {}
                    }

                    None
                });
            }
            Ok(_) => {}
            Err(e) => {
                error!("Connection error: {}", e);
                // Cleanup session if needed, or keep it for reconnection
                break;
            }
        }
    }
}
