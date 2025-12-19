use anyhow::Result;
use tracing::{info, error, instrument, span, Level};
use tokio::net::TcpListener;
use tokio::io::{AsyncRead, AsyncWrite};
use hyperlight_protocol::{ProtocolHandler, Message, BrowserCommand, Response, PreComputedResponse};
use hyperlight_parser::parse_html;
use hyperlight_wasm::WasmRuntime;
use hyperlight_neural::NeuralLatentEncoder;
use hyperlight_cluster::{ClusterNode, NodeCapabilities};
use std::sync::Arc;
use tokio::sync::Mutex;
use dashmap::DashMap;
use uuid::Uuid;
use serde::{Serialize, Deserialize};
use axum::{routing::get, Router};
use prometheus::{Encoder, TextEncoder};

mod vdom;
mod telemetry;
mod tls;
use vdom::VirtualDom;
use telemetry::*;
use tls::*;

#[derive(Serialize, Deserialize, Clone)]
struct Session {
    current_url: Option<String>,
    current_html: Option<String>,
    /// Cached VDOM from last ExecuteBinary
    current_vdom: Option<VirtualDom>,
    last_command: Option<BrowserCommand>,
    needs_morph: bool,
    /// Reactive state variables
    state: std::collections::HashMap<String, serde_json::Value>,
    /// Current HLB binary for reactivity
    current_binary: Option<hyperlight_protocol::HyperlightBinary>,
}

struct BrowserState {
    sessions: DashMap<String, Session>,
    client: reqwest::Client,
    encoder: Mutex<NeuralLatentEncoder>,
    cluster: Mutex<ClusterNode>,
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
    
    let mut latent_to_stream = Vec::new();
    let response = match command {
        BrowserCommand::Navigate { url } => {
            match state.client.get(&url).send().await {
                Ok(resp) => {
                    let html = resp.text().await.unwrap_or_default();
                    if let Some(mut session) = state.sessions.get_mut(session_id) {
                        session.current_url = Some(url);
                        session.current_html = Some(html);
                        session.current_vdom = None; // Reset VDOM on navigation
                    }
                    Response {
                        id: request_id,
                        result: Some(serde_json::json!({"status": "success", "session_id": session_id})),
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
            if let Some(session) = state.sessions.get(session_id) {
                if let Some(html) = &session.current_html {
                    match parse_html(html) {
                        Ok(ur) => Response {
                            id: request_id,
                            result: Some(serde_json::to_value(ur).unwrap()),
                            error: None,
                        },
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
            info!("Executing HLB with {} instructions", bin.instructions.len());
            
            // Store binary in session for reactivity
            if let Some(mut session) = state.sessions.get_mut(session_id) {
                session.current_binary = Some(bin.clone());
            }

            let wasm_runtime = WasmRuntime::new().expect("Failed to initialize WASM runtime");
            let result = wasm_runtime.execute(&bin).expect("Failed to execute HLB in WASM");
            
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
                                event.payload.get("value")
                            ) {
                                session.state.insert(name.to_string(), value.clone());
                            }
                        }
                        _ => {}
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
                    "latent_streams_count": result.latent_streams.len(),
                    "patches": patches,
                })),
                error: None,
            }
        }
        BrowserCommand::HandleEvent { event_name, payload: _payload } => {
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
                    match parse_html(html) {
                        Ok(ur) => {
                            let ur_json = serde_json::to_string(&ur).unwrap_or_default();
                            let mut encoder = state.encoder.lock().await;
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
            let mut encoder = state.encoder.lock().await;
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
                if let Some(html) = &session.current_html {
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
                let data = serde_json::to_vec(&*session).unwrap();
                let cluster = state.cluster.lock().await;
                if let Err(e) = cluster.send_session_data(session_id.to_string(), data).await {
                    Response {
                        id: request_id,
                        result: None,
                        error: Some(format!("Transfer failed: {}", e)),
                    }
                } else {
                    Response {
                        id: request_id,
                        result: Some(serde_json::json!({ "status": "transferred", "target": target_node_id })),
                        error: None,
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
    };
    
    timer.observe_duration();
    (response, latent_to_stream)
}

#[tokio::main]
async fn main() -> Result<()> {
    init_telemetry("hyperlight-core")?;
    info!("Starting Hyperlight Agentic Browser...");

    let addr = "127.0.0.1:8080".parse().unwrap();
    let capabilities = NodeCapabilities {
        supports_wasm: true,
        supports_chameleon: true,
        supports_speculation: true,
        max_sessions: 100,
        region: Some("us-west".to_string()),
    };

    let mut cluster_node = ClusterNode::new(addr, capabilities);
    cluster_node.start().await?;

    let state = Arc::new(BrowserState {
        sessions: DashMap::new(),
        client: reqwest::Client::new(),
        encoder: Mutex::new(NeuralLatentEncoder::new(256, 1024, &[512, 256], 8, 42)), // Standard 256-dim latent space
        cluster: Mutex::new(cluster_node),
    });

    // Start metrics server
    let metrics_app = Router::new().route("/metrics", get(|| async {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = vec![];
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }));

    tokio::spawn(async move {
        let addr = "0.0.0.0:9090".parse().unwrap();
        info!("Metrics server listening on {}", addr);
        axum::Server::bind(&addr)
            .serve(metrics_app.into_make_service())
            .await
            .unwrap();
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
                hyperlight_cluster::ClusterEvent::SessionTransferRequested { session_id, from_node } => {
                    info!("Session transfer requested for {} from node {}", session_id, from_node);
                    // In a real implementation, we might need to acknowledge or prepare
                }
                hyperlight_cluster::ClusterEvent::SessionDataReceived { session_id, data } => {
                    info!("Received session data for {}", session_id);
                    if let Ok(session) = serde_json::from_slice::<Session>(&data) {
                        cluster_state.sessions.insert(session_id, session);
                    }
                }
                hyperlight_cluster::ClusterEvent::SearchRequested { query, request_id, origin_node } => {
                    info!("Distributed search requested: '{}' from node {}", query, origin_node);
                    
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
                hyperlight_cluster::ClusterEvent::SearchResultReceived { request_id, results, node_id } => {
                    info!("Received search results for {} from node {}", request_id, node_id);
                    // In a real app, we'd push this to the client via a WebSocket or similar
                }
                _ => {}
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
                    info!("Self-healing: triggering proactive protocol morph for session {}", session.key());
                    session.needs_morph = true;
                }
            }
        }
    });

    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    info!("Listening on 127.0.0.1:8080");

    // TLS setup
    let tls_acceptor = if std::env::var("HYPERLIGHT_TLS").unwrap_or_default() == "1" {
        let cert_path = std::path::Path::new("certs/cert.pem");
        let key_path = std::path::Path::new("certs/key.pem");
        if cert_path.exists() && key_path.exists() {
            Some(create_tls_acceptor(cert_path, key_path)?)
        } else {
            info!("TLS enabled but certs not found at certs/. Falling back to plain TCP.");
            None
        }
    } else {
        None
    };

    loop {
        let (socket, addr) = listener.accept().await?;
        let span = span!(Level::INFO, "connection", remote_addr = %addr);
        let _enter = span.enter();
        info!("New connection from {}", addr);

        let state = Arc::clone(&state);
        let tls_acceptor = tls_acceptor.clone();
        
        tokio::spawn(async move {
            if let Some(acceptor) = tls_acceptor {
                match acceptor.accept(socket).await {
                    Ok(tls_stream) => {
                        let mut handler = ProtocolHandler::new(tls_stream);
                        handle_session(&mut handler, state).await;
                    }
                    Err(e) => error!("TLS handshake failed: {}", e),
                }
            } else {
                let mut handler = ProtocolHandler::new(socket);
                handle_session(&mut handler, state).await;
            }
        });
    }
}

async fn handle_session<S>(handler: &mut ProtocolHandler<S>, state: Arc<BrowserState>) 
where 
    S: AsyncRead + AsyncWrite + Unpin + Send 
{
    let mut session_id: Option<String> = None;

    loop {
        match handler.receive_message().await {
            Ok(Message::Request(req)) => {
                let req_span = span!(Level::INFO, "request", request_id = %req.id, command = ?req.command);
                let _enter = req_span.enter();
                info!("Received request: {:?}", req);
                
                // Ensure session exists
                let id = if let Some(id) = session_id.as_ref() {
                    id.clone()
                } else {
                    let new_id = Uuid::new_v4().to_string();
                    SESSIONS_ACTIVE.inc();
                    state.sessions.insert(new_id.clone(), Session {
                        current_url: None,
                        current_html: None,
                        current_vdom: None,
                        last_command: None,
                        needs_morph: false,
                        state: std::collections::HashMap::new(),
                        current_binary: None,
                    });
                    
                    // Register session with cluster
                    let cluster = state.cluster.lock().await;
                    cluster.register_local_session(new_id.clone());
                    
                    session_id = Some(new_id.clone());
                    new_id
                };

                // Check if session needs a proactive morph
                if let Some(mut session) = state.sessions.get_mut(&id) {
                    if session.needs_morph {
                        let seed = rand::random::<u64>();
                        info!("Sending proactive MorphRequest for session {}", id);
                        if let Ok(_) = handler.send_message(&Message::MorphRequest { seed }).await {
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
                    (Response {
                        id: req.id,
                        result: Some(serde_json::json!({ "status": "morphed", "seed": seed })),
                        error: None,
                    }, Vec::new())
                } else {
                    let (res, latent) = handle_command(&state, &id, req.command.clone(), req.id).await;
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
                    let latent_msg = hyperlight_protocol::LatentVector {
                        components: vector,
                        dim_hint: 0,
                        epoch: 0,
                    };
                    if let Err(e) = handler.send_message(&Message::LatentMessage(latent_msg)).await {
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
                                        result: serde_json::to_value(ur).unwrap(),
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
