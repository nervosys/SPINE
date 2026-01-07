// Allow dead code for browser GUI features
#![allow(dead_code)]

use eframe::egui;
use spine_agent::AgentClient;
use spine_human::HumanTranspiler;
use spine_wasm::WasmElement;
use spine_protocol::VDomPatch;
use spine_cluster::ClusterClient;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::{Mutex, mpsc};

enum BrowserEvent {
    StatusChanged(String),
    ContentUpdated(String),
    HlsUpdated(String),
    ElementsUpdated(Vec<WasmElement>),
    PatchesApplied(Vec<VDomPatch>),
    LatentUpdated(Vec<f32>),
    KnowledgeUpdated(Vec<serde_json::Value>),
    HistoryUpdated(Vec<spine_protocol::BrowserCommand>),
    CapabilitiesUpdated(Vec<String>),
    ReasoningUpdated(Vec<spine_human::AgentAction>),
    PlansUpdated(Vec<spine_protocol::SwarmPlan>),
    NeuralStatsUpdated(spine_agentic::TransmissionResult),
    MemoryStateUpdated(String, f32),
    SpeechActsUpdated(Vec<spine_agentic::Performative>),
    Error(String),
}

struct SpineBrowser {
    url: String,
    search_query: String,
    hls_input: String,
    content: String,
    hls_preview: String,
    status: String,
    elements: Vec<WasmElement>,
    latent_vector: Vec<f32>,
    knowledge_base: Vec<serde_json::Value>,
    session_history: Vec<spine_protocol::BrowserCommand>,
    current_capabilities: Vec<String>,
    suggested_actions: Vec<spine_human::AgentAction>,
    new_memory_key: String,
    new_memory_value: String,
    target_agent_id: String,
    task_description: String,
    proposal_key: String,
    proposal_value: String,
    proposal_tags: String,
    swarm_goal: String,
    active_plans: Vec<spine_protocol::SwarmPlan>,
    neural_stats: Option<spine_agentic::TransmissionResult>,
    miras_variant: String,
    surprise_level: f32,
    selected_domain: spine_agentic::ProtocolDomain,
    speech_acts: Vec<spine_agentic::Performative>,
    comms_target_id: String,
    comms_performative: String,
    comms_content: String,
    agent: Arc<Mutex<Option<AgentClient<tokio::net::TcpStream>>>>,
    cluster_client: ClusterClient,
    rt: Runtime,
    human_mode: bool,
    history: Vec<String>,
    show_hls: bool,
    show_latent: bool,
    show_knowledge: bool,
    show_history: bool,
    show_reasoning: bool,
    show_swarm: bool,
    show_consensus: bool,
    show_planning: bool,
    show_neural: bool,
    show_communication: bool,
    autonomous_mode: bool,
    event_tx: mpsc::Sender<BrowserEvent>,
    event_rx: mpsc::Receiver<BrowserEvent>,
}

impl SpineBrowser {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            url: "https://example.com".to_string(),
            search_query: String::new(),
            hls_input: "on_mount -> {\n  print(\"Hello from HLS!\");\n}".to_string(),
            content: "Welcome to SPINE Browser".to_string(),
            hls_preview: String::new(),
            status: "Disconnected".to_string(),
            elements: Vec::new(),
            latent_vector: Vec::new(),
            knowledge_base: Vec::new(),
            session_history: Vec::new(),
            current_capabilities: Vec::new(),
            suggested_actions: Vec::new(),
            new_memory_key: String::new(),
            new_memory_value: String::new(),
            target_agent_id: String::new(),
            task_description: String::new(),
            proposal_key: String::new(),
            proposal_value: String::new(),
            proposal_tags: String::new(),
            swarm_goal: String::new(),
            active_plans: Vec::new(),
            neural_stats: None,
            miras_variant: "MONETA".to_string(),
            surprise_level: 0.0,
            selected_domain: spine_agentic::ProtocolDomain::BulkData,
            speech_acts: Vec::new(),
            comms_target_id: String::new(),
            comms_performative: "Inform".to_string(),
            comms_content: String::new(),
            agent: Arc::new(Mutex::new(None)),
            cluster_client: ClusterClient::new(vec!["127.0.0.1:8080".parse().unwrap()]),
            rt: Runtime::new().unwrap(),
            human_mode: true,
            history: Vec::new(),
            show_hls: true,
            show_latent: false,
            show_knowledge: false,
            show_history: false,
            show_reasoning: true,
            show_swarm: false,
            show_consensus: false,
            show_planning: false,
            show_neural: true,
            show_communication: true,
            autonomous_mode: false,
            event_tx: tx,
            event_rx: rx,
        }
    }

    fn connect(&mut self) {
        let agent_clone = self.agent.clone();
        let tx = self.event_tx.clone();
        let addr = self.cluster_client.get_connection(None)
            .unwrap_or_else(|| "127.0.0.1:8080".parse().unwrap());
        
        self.rt.spawn(async move {
            let _ = tx.send(BrowserEvent::StatusChanged(format!("Connecting to {}...", addr))).await;
            match AgentClient::connect(&addr.to_string()).await {
                Ok(mut client) => {
                    let (mut latent_rx, _) = client.start_listener().await;
                    let tx_latent = tx.clone();
                    
                    tokio::spawn(async move {
                        while let Some(vec) = latent_rx.recv().await {
                            let _ = tx_latent.send(BrowserEvent::LatentUpdated(vec)).await;
                        }
                    });

                    let mut lock = agent_clone.lock().await;
                    *lock = Some(client);
                    let _ = tx.send(BrowserEvent::StatusChanged("Connected".to_string())).await;
                }
                Err(e) => {
                    let _ = tx.send(BrowserEvent::Error(format!("Connection failed: {}", e))).await;
                }
            }
        });
    }

    fn navigate(&mut self) {
        let url = self.url.clone();
        let agent_clone = self.agent.clone();
        let human_mode = self.human_mode;
        let tx = self.event_tx.clone();
        
        self.history.push(url.clone());
        
        self.rt.spawn(async move {
            let mut lock = agent_clone.lock().await;
            let agent: &mut AgentClient<tokio::net::TcpStream> = match lock.as_mut() {
                Some(a) => a,
                None => return,
            };
            
            let _ = tx.send(BrowserEvent::StatusChanged(format!("Navigating to {}...", url))).await;
            
            match agent.navigate(&url).await {
                    Ok(_) => {
                        if human_mode {
                            match agent.get_raw_html().await {
                                Ok(html) => {
                                    match HumanTranspiler::transpile(&html, "", "") {
                                        Ok(bin) => {
                                            let _ = tx.send(BrowserEvent::HlsUpdated(format!("// Transpiled HLS for {}\n// Instructions: {}\n", url, bin.instructions.len()))).await;
                                            let _ = tx.send(BrowserEvent::ContentUpdated("Transpilation successful. Executing binary...".to_string())).await;
                                            
                                            if let Ok(res) = agent.execute_binary(bin).await {
                                                let _ = tx.send(BrowserEvent::ContentUpdated(format!("Execution Result: {}", res))).await;
                                                if let Some(count) = res.get("latent_streams_count") {
                                                    let _ = tx.send(BrowserEvent::StatusChanged(format!("Receiving {} latent streams...", count))).await;
                                                }
                                                if let Some(elements_val) = res.get("elements") {
                                                    if let Ok(elements) = serde_json::from_value::<Vec<WasmElement>>(elements_val.clone()) {
                                                        let _ = tx.send(BrowserEvent::ElementsUpdated(elements)).await;
                                                    }
                                                }
                                                if let Some(patches_val) = res.get("patches") {
                                                    if let Ok(patches) = serde_json::from_value::<Vec<VDomPatch>>(patches_val.clone()) {
                                                        if !patches.is_empty() {
                                                            let _ = tx.send(BrowserEvent::PatchesApplied(patches)).await;
                                                        }
                                                    }
                                                }
                                            }

                                            // Refresh knowledge and history
                                            if let Ok(kb) = agent.query_knowledge("*", vec![], 100).await {
                                                let _ = tx.send(BrowserEvent::KnowledgeUpdated(kb)).await;
                                            }
                                            if let Ok(hist) = agent.get_history().await {
                                                let _ = tx.send(BrowserEvent::HistoryUpdated(hist)).await;
                                            }
                                            if let Ok(caps) = agent.get_capabilities().await {
                                                let _ = tx.send(BrowserEvent::CapabilitiesUpdated(caps)).await;
                                            }

                                            // Also trigger reasoning in human mode
                                            if let Ok(ur) = agent.get_ur().await {
                                                let engine = spine_human::ReasoningEngine::new();
                                                let actions = engine.suggest_actions(&ur);
                                                let _ = tx.send(BrowserEvent::ReasoningUpdated(actions)).await;
                                            }
                                        }
                                        Err(e) => {
                                            let _ = tx.send(BrowserEvent::Error(format!("Transpilation failed: {}", e))).await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = tx.send(BrowserEvent::Error(format!("Failed to get raw HTML: {}", e))).await;
                                }
                            }
                        } else {
                            match agent.get_ur().await {
                                Ok(ur) => {
                                    let _ = tx.send(BrowserEvent::ContentUpdated(format!("Unified Representation:\n{:#?}", ur))).await;
                                    
                                    // Trigger Reasoning Engine
                                    let engine = spine_human::ReasoningEngine::new();
                                    let actions = engine.suggest_actions(&ur);
                                    let _ = tx.send(BrowserEvent::ReasoningUpdated(actions)).await;
                                }
                                Err(e) => {
                                    let _ = tx.send(BrowserEvent::Error(format!("Failed to get UR: {}", e))).await;
                                }
                            }
                        }
                        let _ = tx.send(BrowserEvent::StatusChanged("Ready".to_string())).await;
                    }
                    Err(e) => {
                        let _ = tx.send(BrowserEvent::Error(format!("Navigation failed: {}", e))).await;
                    }
                }
        });
    }

    fn search(&mut self) {
        let query = self.search_query.clone();
        let agent_clone = self.agent.clone();
        let tx = self.event_tx.clone();
        
        self.rt.spawn(async move {
            let mut lock = agent_clone.lock().await;
            let agent: &mut AgentClient<tokio::net::TcpStream> = match lock.as_mut() {
                Some(a) => a,
                None => return,
            };
            
            let _ = tx.send(BrowserEvent::StatusChanged(format!("Searching for '{}'...", query))).await;
            match agent.search(&query).await {
                Ok(results) => {
                    let _ = tx.send(BrowserEvent::ContentUpdated(format!("Search Results:\n{}", serde_json::to_string_pretty(&results).unwrap()))).await;
                    let _ = tx.send(BrowserEvent::StatusChanged("Search complete".to_string())).await;

                    // Refresh knowledge and history
                    if let Ok(kb) = agent.query_knowledge("*", vec![], 100).await {
                        let _ = tx.send(BrowserEvent::KnowledgeUpdated(kb)).await;
                    }
                    if let Ok(hist) = agent.get_history().await {
                        let _ = tx.send(BrowserEvent::HistoryUpdated(hist)).await;
                    }
                    if let Ok(caps) = agent.get_capabilities().await {
                        let _ = tx.send(BrowserEvent::CapabilitiesUpdated(caps)).await;
                    }
                }
                Err(e) => {
                    let _ = tx.send(BrowserEvent::Error(format!("Search failed: {}", e))).await;
                }
            }
        });
    }

    fn swarm_search(&mut self) {
        let query = self.search_query.clone();
        let agent_clone = self.agent.clone();
        let tx = self.event_tx.clone();
        
        self.rt.spawn(async move {
            let mut lock = agent_clone.lock().await;
            let agent: &mut AgentClient<tokio::net::TcpStream> = match lock.as_mut() {
                Some(a) => a,
                None => return,
            };
            
            let _ = tx.send(BrowserEvent::StatusChanged(format!("Initiating swarm search for '{}'...", query))).await;
            match agent.swarm_search(&query, 2).await {
                Ok(_) => {
                    let _ = tx.send(BrowserEvent::StatusChanged("Swarm search initiated across cluster".to_string())).await;
                }
                Err(e) => {
                    let _ = tx.send(BrowserEvent::Error(format!("Swarm search failed: {}", e))).await;
                }
            }
        });
    }

    fn delegate_task(&mut self) {
        let target_id_str = self.target_agent_id.clone();
        let task = self.task_description.clone();
        let agent_clone = self.agent.clone();
        let tx = self.event_tx.clone();
        
        self.rt.spawn(async move {
            let mut lock = agent_clone.lock().await;
            let agent: &mut AgentClient<tokio::net::TcpStream> = match lock.as_mut() {
                Some(a) => a,
                None => return,
            };
            
            let target_id = uuid::Uuid::parse_str(&target_id_str).ok();
            
            let _ = tx.send(BrowserEvent::StatusChanged(format!("Delegating task to {:?}...", target_id))).await;
            match agent.delegate_task(&task, target_id).await {
                Ok(_) => {
                    let _ = tx.send(BrowserEvent::StatusChanged(format!("Task delegated to {:?}", target_id))).await;
                }
                Err(e) => {
                    let _ = tx.send(BrowserEvent::Error(format!("Delegation failed: {}", e))).await;
                }
            }
        });
    }

    fn propose_knowledge(&mut self) {
        let key = self.proposal_key.clone();
        let value_str = self.proposal_value.clone();
        let value = serde_json::Value::String(value_str);
        let tags: Vec<String> = self.proposal_tags.split(',').map(|s| s.trim().to_string()).collect();
        let agent_clone = self.agent.clone();
        let tx = self.event_tx.clone();

        self.rt.spawn(async move {
            let mut lock = agent_clone.lock().await;
            let agent: &mut AgentClient<tokio::net::TcpStream> = match lock.as_mut() {
                Some(a) => a,
                None => return,
            };

            let _ = tx.send(BrowserEvent::StatusChanged(format!("Proposing knowledge: {}...", key))).await;
            match agent.propose_knowledge(&key, value, tags).await {
                Ok(_) => {
                    let _ = tx.send(BrowserEvent::StatusChanged("Knowledge proposed to cluster".to_string())).await;
                }
                Err(e) => {
                    let _ = tx.send(BrowserEvent::Error(format!("Proposal failed: {}", e))).await;
                }
            }
        });
    }

    fn create_swarm_plan(&mut self) {
        let goal = self.swarm_goal.clone();
        let agent_clone = self.agent.clone();
        let tx = self.event_tx.clone();

        self.rt.spawn(async move {
            let mut lock = agent_clone.lock().await;
            let agent: &mut AgentClient<tokio::net::TcpStream> = match lock.as_mut() {
                Some(a) => a,
                None => return,
            };

            let _ = tx.send(BrowserEvent::StatusChanged(format!("Creating swarm plan for: {}...", goal))).await;
            match agent.create_swarm_plan(&goal).await {
                Ok(plan_id) => {
                    let _ = tx.send(BrowserEvent::StatusChanged(format!("Swarm plan created: {}", plan_id))).await;
                }
                Err(e) => {
                    let _ = tx.send(BrowserEvent::Error(format!("Planning failed: {}", e))).await;
                }
            }
        });
    }

    fn refresh_agentic_state(&mut self) {
        let agent_clone = self.agent.clone();
        let tx = self.event_tx.clone();
        
        self.rt.spawn(async move {
            let mut lock = agent_clone.lock().await;
            let agent = match lock.as_mut() {
                Some(a) => a,
                None => return,
            };
            
            if let Ok(state) = agent.get_agentic_state().await {
                if let Some(variant) = state["miras_variant"].as_str() {
                    let surprise = state["surprise_level"].as_f64().unwrap_or(0.0) as f32;
                    let _ = tx.send(BrowserEvent::MemoryStateUpdated(variant.to_string(), surprise)).await;
                }
                if let Some(acts_val) = state["speech_acts"].as_array() {
                    if let Ok(acts) = serde_json::from_value::<Vec<spine_agentic::Performative>>(serde_json::Value::Array(acts_val.clone())) {
                        let _ = tx.send(BrowserEvent::SpeechActsUpdated(acts)).await;
                    }
                }
            }
        });
    }

    fn send_speech_act(&mut self) {
        let target_id_str = self.comms_target_id.clone();
        let performative = self.comms_performative.clone();
        let content = self.comms_content.clone();
        let agent_clone = self.agent.clone();
        let tx = self.event_tx.clone();
        
        self.rt.spawn(async move {
            let target_id = match uuid::Uuid::parse_str(&target_id_str) {
                Ok(id) => id,
                Err(_) => {
                    let _ = tx.send(BrowserEvent::Error("Invalid Target Agent ID".to_string())).await;
                    return;
                }
            };

            let mut lock = agent_clone.lock().await;
            let agent = match lock.as_mut() {
                Some(a) => a,
                None => return,
            };
            
            if let Err(e) = agent.send_speech_act(target_id, &performative, &content).await {
                let _ = tx.send(BrowserEvent::Error(format!("Failed to send speech act: {}", e))).await;
            } else {
                let _ = tx.send(BrowserEvent::StatusChanged("Speech act sent".to_string())).await;
            }
        });
        self.comms_content.clear();
    }

    fn transfer(&mut self) {
        let agent_clone = self.agent.clone();
        let tx = self.event_tx.clone();
        let target_node = uuid::Uuid::new_v4();
        
        self.rt.spawn(async move {
            let mut lock = agent_clone.lock().await;
            let agent: &mut AgentClient<tokio::net::TcpStream> = match lock.as_mut() {
                Some(a) => a,
                None => return,
            };
            
            let _ = tx.send(BrowserEvent::StatusChanged(format!("Transferring session to {}...", target_node))).await;
            match agent.transfer_session(target_node).await {
                Ok(_) => {
                    let _ = tx.send(BrowserEvent::StatusChanged("Transfer initiated".to_string())).await;

                    // Refresh knowledge and history
                    if let Ok(kb) = agent.query_knowledge("*", vec![], 100).await {
                        let _ = tx.send(BrowserEvent::KnowledgeUpdated(kb)).await;
                    }
                    if let Ok(hist) = agent.get_history().await {
                        let _ = tx.send(BrowserEvent::HistoryUpdated(hist)).await;
                    }
                    if let Ok(caps) = agent.get_capabilities().await {
                        let _ = tx.send(BrowserEvent::CapabilitiesUpdated(caps)).await;
                    }
                }
                Err(e) => {
                    let _ = tx.send(BrowserEvent::Error(format!("Transfer failed: {}", e))).await;
                }
            }
        });
    }
    fn apply_patches(&mut self, patches: Vec<VDomPatch>) {
        for patch in patches {
            match patch {
                VDomPatch::Create { id, tag } => {
                    if !self.elements.iter().any(|e| e.id == id) {
                        self.elements.push(WasmElement {
                            id,
                            tag,
                            attributes: std::collections::HashMap::new(),
                            parent_id: None,
                        });
                    }
                }
                VDomPatch::Remove { id } => {
                    self.elements.retain(|e| e.id != id);
                }
                VDomPatch::SetAttr { id, key, value } => {
                    if let Some(elem) = self.elements.iter_mut().find(|e| e.id == id) {
                        elem.attributes.insert(key, value);
                    }
                }
                VDomPatch::RemoveAttr { id, key } => {
                    if let Some(elem) = self.elements.iter_mut().find(|e| e.id == id) {
                        elem.attributes.remove(&key);
                    }
                }
                VDomPatch::AppendChild { parent_id, child_id } => {
                    if let Some(elem) = self.elements.iter_mut().find(|e| e.id == child_id) {
                        elem.parent_id = Some(parent_id);
                    }
                }
                VDomPatch::RemoveChild { parent_id: _, child_id } => {
                    if let Some(elem) = self.elements.iter_mut().find(|e| e.id == child_id) {
                        elem.parent_id = None;
                    }
                }
                VDomPatch::ReorderChildren { .. } => {
                    // In this simple renderer, order is determined by the elements list
                    // A more complex implementation would use the order hint
                }
            }
        }
    }

    fn render_elements(&self, ui: &mut egui::Ui, parent_id: Option<u32>) {
        for elem in self.elements.iter().filter(|e| e.parent_id == parent_id) {
            match elem.tag.as_str() {
                "Heading" => {
                    ui.heading(elem.attributes.get("text").cloned().unwrap_or_else(|| 
                        elem.attributes.get("content").cloned().unwrap_or_default()));
                }
                "Button" => {
                    let text = elem.attributes.get("text").cloned().unwrap_or_else(|| 
                        elem.attributes.get("content").cloned().unwrap_or_else(|| "Button".to_string()));
                    if ui.button(text).clicked() {
                        let agent_clone = self.agent.clone();
                        let element_id = elem.id;
                        let tx = self.event_tx.clone();
                        
                        self.rt.spawn(async move {
                            let mut lock = agent_clone.lock().await;
                            let agent: &mut AgentClient<tokio::net::TcpStream> = match lock.as_mut() {
                                Some(a) => a,
                                None => return,
                            };
                            
                            let _ = tx.send(BrowserEvent::StatusChanged(format!("Triggering event on {}...", element_id))).await;
                            match agent.handle_event(element_id, "click", serde_json::Value::Null).await {
                                Ok(patches) => {
                                    let _ = tx.send(BrowserEvent::PatchesApplied(patches)).await;
                                    let _ = tx.send(BrowserEvent::StatusChanged("Event handled".to_string())).await;
                                }
                                Err(e) => {
                                    let _ = tx.send(BrowserEvent::Error(format!("Event failed: {}", e))).await;
                                }
                            }
                        });
                    }
                }
                "Link" => {
                    let text = elem.attributes.get("text").cloned().unwrap_or_else(|| 
                        elem.attributes.get("content").cloned().unwrap_or_else(|| "Link".to_string()));
                    let href = elem.attributes.get("href").cloned().unwrap_or_default();
                    ui.hyperlink_to(text, href);
                }
                "text" | "Text" => {
                    let content = elem.attributes.get("content").cloned().unwrap_or_else(|| 
                        elem.attributes.get("text").cloned().unwrap_or_default());
                    ui.label(content);
                }
                "Image" => {
                    ui.label(format!("[Image: {}]", elem.attributes.get("src").cloned().unwrap_or_default()));
                }
                "Input" => {
                    let mut val = elem.attributes.get("value").cloned().unwrap_or_default();
                    ui.text_edit_singleline(&mut val);
                }
                _ => {
                    // Generic container
                    ui.vertical(|ui| {
                        self.render_elements(ui, Some(elem.id));
                    });
                }
            }
        }
    }
}

impl eframe::App for SpineBrowser {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process events
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                BrowserEvent::StatusChanged(s) => self.status = s,
                BrowserEvent::ContentUpdated(c) => self.content = c,
                BrowserEvent::HlsUpdated(h) => self.hls_preview = h,
                BrowserEvent::ElementsUpdated(e) => self.elements = e,
                BrowserEvent::PatchesApplied(patches) => {
                    self.apply_patches(patches);
                }
                BrowserEvent::LatentUpdated(l) => self.latent_vector = l,
                BrowserEvent::KnowledgeUpdated(kb) => self.knowledge_base = kb,
                BrowserEvent::HistoryUpdated(hist) => self.session_history = hist,
                BrowserEvent::CapabilitiesUpdated(caps) => self.current_capabilities = caps,
                BrowserEvent::ReasoningUpdated(actions) => self.suggested_actions = actions,
                BrowserEvent::PlansUpdated(plans) => self.active_plans = plans,
                BrowserEvent::NeuralStatsUpdated(stats) => self.neural_stats = Some(stats),
                BrowserEvent::MemoryStateUpdated(variant, surprise) => {
                    self.miras_variant = variant;
                    self.surprise_level = surprise;
                }
                BrowserEvent::SpeechActsUpdated(acts) => self.speech_acts = acts,
                BrowserEvent::Error(e) => {
                    self.status = format!("Error: {}", e);
                    self.content = format!("Error: {}", e);
                }
            }
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("SPINE Browser");
                ui.separator();
                
                if ui.button("⬅").clicked() && self.history.len() > 1 {
                    self.history.pop();
                    self.url = self.history.last().cloned().unwrap_or_default();
                    self.navigate();
                }

                ui.label("URL:");
                let res = ui.text_edit_singleline(&mut self.url);
                if res.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.navigate();
                }
                
                if ui.button("Go").clicked() {
                    self.navigate();
                }
                
                if ui.button("Connect").clicked() {
                    self.connect();
                }

                if ui.button("Ping").clicked() {
                    let agent_clone = self.agent.clone();
                    let tx = self.event_tx.clone();
                    self.rt.spawn(async move {
                        let mut lock = agent_clone.lock().await;
                        let agent = match lock.as_mut() {
                            Some(a) => a,
                            None => return,
                        };
                        
                        match agent.ping().await {
                            Ok(latency) => {
                                let _ = tx.send(BrowserEvent::StatusChanged(format!("Latency: {}ms", latency))).await;
                            }
                            Err(e) => {
                                let _ = tx.send(BrowserEvent::Error(format!("Ping failed: {}", e))).await;
                            }
                        }
                    });
                }

                ui.separator();
                ui.label("Search:");
                let res = ui.text_edit_singleline(&mut self.search_query);
                if res.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.search();
                }
                if ui.button("🔍").clicked() {
                    self.search();
                }
                if ui.button("🐝 Swarm").clicked() {
                    self.swarm_search();
                }

                if ui.button("Transfer").clicked() {
                    self.transfer();
                }
                ui.separator();

                if ui.button("Latent").clicked() {
                    let agent_clone = self.agent.clone();
                    let tx = self.event_tx.clone();
                    self.rt.spawn(async move {
                        let mut lock = agent_clone.lock().await;
                        let agent: &mut AgentClient<tokio::net::TcpStream> = match lock.as_mut() {
                            Some(a) => a,
                            None => return,
                        };
                        
                        let _ = tx.send(BrowserEvent::StatusChanged("Fetching latent representation...".to_string())).await;
                        match agent.get_latent_ur(128).await {
                            Ok(latent) => {
                                let _ = tx.send(BrowserEvent::LatentUpdated(latent)).await;
                                let _ = tx.send(BrowserEvent::StatusChanged("Latent vector received".to_string())).await;
                            }
                            Err(e) => {
                                let _ = tx.send(BrowserEvent::Error(format!("Latent fetch failed: {}", e))).await;
                            }
                        }
                    });
                }

                if ui.button("Morph").clicked() {
                    let agent_clone = self.agent.clone();
                    let tx = self.event_tx.clone();
                    self.rt.spawn(async move {
                        let mut lock = agent_clone.lock().await;
                        let agent: &mut AgentClient<tokio::net::TcpStream> = match lock.as_mut() {
                            Some(a) => a,
                            None => return,
                        };
                        
                        let _ = tx.send(BrowserEvent::StatusChanged("Morphing protocol...".to_string())).await;
                        match agent.morph().await {
                            Ok(_) => {
                                let _ = tx.send(BrowserEvent::StatusChanged("Protocol Morphed".to_string())).await;
                            }
                            Err(e) => {
                                let _ = tx.send(BrowserEvent::Error(format!("Morph failed: {}", e))).await;
                            }
                        }
                    });
                }

                if ui.button("Decoy").clicked() {
                    let agent_clone = self.agent.clone();
                    let tx = self.event_tx.clone();
                    self.rt.spawn(async move {
                        let mut lock = agent_clone.lock().await;
                        let agent: &mut AgentClient<tokio::net::TcpStream> = match lock.as_mut() {
                            Some(a) => a,
                            None => return,
                        };
                        
                        let _ = tx.send(BrowserEvent::StatusChanged("Injecting decoy traffic...".to_string())).await;
                        match agent.handler.send_decoy().await {
                            Ok(_) => {
                                let _ = tx.send(BrowserEvent::StatusChanged("Decoy Injected".to_string())).await;
                            }
                            Err(e) => {
                                let _ = tx.send(BrowserEvent::Error(format!("Decoy failed: {}", e))).await;
                            }
                        }
                    });
                }
                
                ui.checkbox(&mut self.human_mode, "Human Mode");
                ui.checkbox(&mut self.show_hls, "HLS");
                ui.checkbox(&mut self.show_latent, "Latent");
                ui.checkbox(&mut self.show_knowledge, "Memory");
                ui.checkbox(&mut self.show_history, "History");
                ui.checkbox(&mut self.show_reasoning, "Reasoning");
                ui.checkbox(&mut self.show_swarm, "Swarm");
                ui.checkbox(&mut self.show_consensus, "Consensus");
                ui.checkbox(&mut self.show_planning, "Planning");
                ui.checkbox(&mut self.show_neural, "Neural");
                ui.checkbox(&mut self.show_communication, "Comms");

                if ui.checkbox(&mut self.autonomous_mode, "Auto").changed() {
                    let enabled = self.autonomous_mode;
                    let agent_clone = self.agent.clone();
                    let tx = self.event_tx.clone();
                    self.rt.spawn(async move {
                        let mut lock = agent_clone.lock().await;
                        let agent = match lock.as_mut() {
                            Some(a) => a,
                            None => return,
                        };
                        
                        if let Err(e) = agent.set_autonomous_mode(enabled).await {
                            let _ = tx.send(BrowserEvent::Error(format!("Failed to set auto mode: {}", e))).await;
                        }
                    });
                }
            });
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Status: {}", self.status));
                ui.separator();
                
                if let Ok(agent_lock) = self.agent.try_lock() {
                    if let Some(agent) = agent_lock.as_ref() {
                        let agent: &AgentClient<tokio::net::TcpStream> = agent;
                        let stats = agent.handler.speculation_stats.clone();
                        ui.label(format!("Speculation: Hits: {} / Preds: {} ({:.1}%)", 
                            stats.output_hits, 
                            stats.output_predictions,
                            if stats.output_predictions > 0 {
                                (stats.output_hits as f32 / stats.output_predictions as f32) * 100.0
                            } else {
                                0.0
                            }
                        ));
                        ui.separator();
                        ui.label(format!("Bandwidth Saved: {} bytes", stats.bytes_saved));
                    }
                }
                
                ui.separator();
                ui.label(format!("History: {} pages", self.history.len()));
                ui.separator();
                ui.label("Capabilities:");
                for cap in &self.current_capabilities {
                    let color = match cap.as_str() {
                        "network" => egui::Color32::from_rgb(100, 100, 255),
                        "storage" => egui::Color32::from_rgb(100, 255, 100),
                        "memory" => egui::Color32::from_rgb(255, 255, 100),
                        "search" => egui::Color32::from_rgb(255, 100, 255),
                        _ => egui::Color32::GRAY,
                    };
                    ui.colored_label(color, format!("[{}]", cap));
                }
            });
        });

        if self.show_hls {
            egui::SidePanel::right("hls_panel").resizable(true).show(ctx, |ui| {
                ui.heading("HLS Script");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut self.hls_input)
                        .font(egui::TextStyle::Monospace)
                        .code_editor());
                });
                
                if ui.button("Execute HLS").clicked() {
                    let script = self.hls_input.clone();
                    let agent_clone = self.agent.clone();
                    let tx = self.event_tx.clone();
                    self.rt.spawn(async move {
                        let mut lock = agent_clone.lock().await;
                        let agent = match lock.as_mut() {
                            Some(a) => a,
                            None => return,
                        };
                        
                        let _ = tx.send(BrowserEvent::StatusChanged("Compiling and executing HLS...".to_string())).await;
                        match agent.execute_hls(&script).await {
                            Ok(result) => {
                                let _ = tx.send(BrowserEvent::ElementsUpdated(result.elements)).await;
                                let _ = tx.send(BrowserEvent::StatusChanged("HLS executed successfully".to_string())).await;
                                
                                // Handle autonomous actions
                                for action in result.actions {
                                    match action {
                                        spine_wasm::WasmAction::Navigate(url) => {
                                            let _ = tx.send(BrowserEvent::StatusChanged(format!("HLS Navigating to {}...", url))).await;
                                            // In a real browser, we'd trigger a navigation here
                                        }
                                        spine_wasm::WasmAction::Search(query) => {
                                            let _ = tx.send(BrowserEvent::StatusChanged(format!("HLS Searching for {}...", query))).await;
                                        }
                                        spine_wasm::WasmAction::StoreKnowledge { key, .. } => {
                                            let _ = tx.send(BrowserEvent::StatusChanged(format!("HLS Storing knowledge: {}...", key))).await;
                                        }
                                        spine_wasm::WasmAction::QueryKnowledge { query, .. } => {
                                            let _ = tx.send(BrowserEvent::StatusChanged(format!("HLS Querying knowledge: {}...", query))).await;
                                        }
                                        spine_wasm::WasmAction::Reason(query) => {
                                            let _ = tx.send(BrowserEvent::StatusChanged(format!("HLS Reasoning about {}...", query))).await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(BrowserEvent::Error(format!("HLS execution failed: {}", e))).await;
                            }
                        }
                    });
                }

                ui.separator();
                ui.heading("HLS Preview (Compiled)");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut self.hls_preview)
                        .font(egui::TextStyle::Monospace)
                        .code_editor());
                });
            });
        }

        if self.show_latent {
            egui::SidePanel::left("latent_panel").resizable(true).show(ctx, |ui| {
                ui.heading("Neural Latent Space");
                ui.label(format!("Dimensions: {}", self.latent_vector.len()));
                
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for &val in &self.latent_vector {
                            let color = if val > 0.0 {
                                egui::Color32::from_rgb(0, (val * 255.0) as u8, 0)
                            } else {
                                egui::Color32::from_rgb((val.abs() * 255.0) as u8, 0, 0)
                            };
                            
                            let (rect, _) = ui.allocate_at_least(egui::vec2(8.0, 8.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect, 0.0, color);
                        }
                    });
                });
                
                if !self.latent_vector.is_empty() {
                    ui.separator();
                    ui.label("Protocol Morphology: Active");
                    ui.label("Encryption: Latent-Implicit");
                }
            });
        }

        if self.show_knowledge {
            egui::SidePanel::left("knowledge_panel").resizable(true).show(ctx, |ui| {
                ui.heading("Knowledge Base");
                ui.separator();
                
                ui.group(|ui| {
                    ui.label("Add Fact:");
                    ui.horizontal(|ui| {
                        ui.label("Key:");
                        ui.text_edit_singleline(&mut self.new_memory_key);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Val:");
                        ui.text_edit_singleline(&mut self.new_memory_value);
                    });
                    if ui.button("Store").clicked() {
                        let key = self.new_memory_key.clone();
                        let val = self.new_memory_value.clone();
                        let agent_clone = self.agent.clone();
                        let tx = self.event_tx.clone();
                        
                        self.rt.spawn(async move {
                            let mut lock = agent_clone.lock().await;
                            let agent = match lock.as_mut() {
                                Some(a) => a,
                                None => return,
                            };
                            
                            if agent.store_knowledge(&key, serde_json::json!(val), vec!["manual".to_string()]).await.is_ok() {
                                if let Ok(kb) = agent.query_knowledge("*", vec![], 100).await {
                                    let _ = tx.send(BrowserEvent::KnowledgeUpdated(kb)).await;
                                }
                            }
                        });
                        self.new_memory_key.clear();
                        self.new_memory_value.clear();
                    }
                });

                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for item in &self.knowledge_base {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(serde_json::to_string_pretty(item).unwrap_or_default());
                                if ui.button("🗑").clicked() {
                                    let key = item["key"].as_str().unwrap_or_default().to_string();
                                    let agent_clone = self.agent.clone();
                                    let tx = self.event_tx.clone();
                                    
                                    self.rt.spawn(async move {
                                        let mut lock = agent_clone.lock().await;
                                        let agent = match lock.as_mut() {
                                            Some(a) => a,
                                            None => return,
                                        };
                                        
                                        if agent.delete_knowledge(&key).await.is_ok() {
                                            if let Ok(kb) = agent.query_knowledge("*", vec![], 100).await {
                                                let _ = tx.send(BrowserEvent::KnowledgeUpdated(kb)).await;
                                            }
                                        }
                                    });
                                }
                            });
                        });
                    }
                });
            });
        }

        if self.show_history {
            egui::SidePanel::right("history_panel").resizable(true).show(ctx, |ui| {
                ui.heading("Session History");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for cmd in &self.session_history {
                        ui.group(|ui| {
                            match cmd {
                                spine_protocol::BrowserCommand::Navigate { url } => ui.label(format!("Navigate: {}", url)),
                                spine_protocol::BrowserCommand::GetUR => ui.label("GetUR"),
                                spine_protocol::BrowserCommand::ExecuteBinary { .. } => ui.label("Execute Binary"),
                                spine_protocol::BrowserCommand::StoreKnowledge { key, value, .. } => ui.label(format!("Store: {} = {}", key, value)),
                                spine_protocol::BrowserCommand::QueryKnowledge { query, .. } => ui.label(format!("Query: {}", query)),
                                spine_protocol::BrowserCommand::GetSessionHistory => ui.label("Get History"),
                                _ => ui.label(format!("{:?}", cmd)),
                            };
                        });
                    }
                });
            });
        }

        if self.show_reasoning {
            egui::SidePanel::right("reasoning_panel").resizable(true).show(ctx, |ui| {
                ui.heading("Agentic Reasoning");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.suggested_actions.is_empty() {
                        ui.label("No actions suggested yet.");
                    }
                    for action in &self.suggested_actions {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(format!("Action: {}", action.action_type));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(format!("{:.0}%", action.confidence * 100.0));
                                });
                            });
                            ui.label(format!("Reason: {}", action.reasoning));
                            if let Some(target) = &action.target_id {
                                ui.label(format!("Target ID: {}", target));
                            }
                            if ui.button("Execute").clicked() {
                                // In a real implementation, this would trigger the action
                                self.status = format!("Executing suggested action: {}", action.action_type);
                            }
                        });
                    }
                });
            });
        }

        if self.show_swarm {
            egui::SidePanel::right("swarm_panel").resizable(true).show(ctx, |ui| {
                ui.heading("Swarm Coordination");
                ui.separator();
                
                ui.group(|ui| {
                    ui.label("Task Delegation:");
                    ui.horizontal(|ui| {
                        ui.label("Target Agent ID:");
                        ui.text_edit_singleline(&mut self.target_agent_id);
                    });
                    ui.label("Task Description:");
                    ui.add(egui::TextEdit::multiline(&mut self.task_description).desired_rows(3));
                    
                    if ui.button("Delegate Task").clicked() {
                        self.delegate_task();
                    }
                });

                ui.separator();
                ui.heading("Cluster Status");
                ui.label("Nodes: Active");
                ui.label("Sync: Real-time");
                ui.label("Protocol: Chameleon");
            });
        }

        if self.show_consensus {
            egui::SidePanel::right("consensus_panel").resizable(true).show(ctx, |ui| {
                ui.heading("Knowledge Consensus");
                ui.separator();
                
                ui.group(|ui| {
                    ui.label("Propose Knowledge:");
                    ui.horizontal(|ui| {
                        ui.label("Key:");
                        ui.text_edit_singleline(&mut self.proposal_key);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Value:");
                        ui.text_edit_singleline(&mut self.proposal_value);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Tags (csv):");
                        ui.text_edit_singleline(&mut self.proposal_tags);
                    });
                    
                    if ui.button("Propose to Cluster").clicked() {
                        self.propose_knowledge();
                    }
                });

                ui.separator();
                ui.heading("Active Proposals");
                ui.label("No active proposals"); // In a real app, this would be dynamic

                ui.separator();
                ui.heading("Shared Knowledge Base");
                for entry in &self.knowledge_base {
                    ui.label(format!("{}: {}", entry["key"], entry["value"]));
                }
            });
        }

        if self.show_planning {
            egui::SidePanel::right("planning_panel").resizable(true).show(ctx, |ui| {
                ui.heading("Swarm Planning");
                ui.separator();
                
                ui.group(|ui| {
                    ui.label("Set Swarm Goal:");
                    ui.add(egui::TextEdit::multiline(&mut self.swarm_goal).desired_rows(2));
                    
                    if ui.button("Generate Swarm Plan").clicked() {
                        self.create_swarm_plan();
                    }
                });

                ui.separator();
                ui.heading("Active Plans");
                if self.active_plans.is_empty() {
                    ui.label("No active plans");
                } else {
                    for plan in &self.active_plans {
                        ui.group(|ui| {
                            ui.label(format!("Goal: {}", plan.goal));
                            ui.label(format!("Status: {:?}", plan.status));
                            ui.separator();
                            for task in &plan.tasks {
                                ui.horizontal(|ui| {
                                    let status_color = match task.status {
                                        spine_protocol::TaskStatus::Pending => egui::Color32::GRAY,
                                        spine_protocol::TaskStatus::InProgress => egui::Color32::YELLOW,
                                        spine_protocol::TaskStatus::Completed => egui::Color32::GREEN,
                                        spine_protocol::TaskStatus::Failed => egui::Color32::RED,
                                    };
                                    ui.label(egui::RichText::new("●").color(status_color));
                                    ui.label(&task.description);
                                });
                                if let Some(node_id) = task.assigned_to {
                                    ui.label(egui::RichText::new(format!("  Assigned to: {}", node_id)).small());
                                }
                                if !task.required_skills.is_empty() {
                                    ui.label(egui::RichText::new(format!("  Skills: {}", task.required_skills.join(", "))).small());
                                }
                            }
                        });
                    }
                }
            });
        }

        if self.show_neural {
            egui::SidePanel::right("neural_panel").resizable(true).show(ctx, |ui| {
                ui.heading("Neural & Memory Dashboard");
                ui.separator();

                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Titans Memory State").strong());
                        if ui.button("🔄").clicked() {
                            self.refresh_agentic_state();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Active MIRAS Variant:");
                        ui.label(egui::RichText::new(&self.miras_variant).color(egui::Color32::from_rgb(0, 255, 255)));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Surprise Level:");
                        ui.add(egui::ProgressBar::new(self.surprise_level).text(format!("{:.2}", self.surprise_level)));
                    });
                });

                ui.separator();
                ui.group(|ui| {
                    ui.label(egui::RichText::new("Neural Protocol Stats").strong());
                    if let Some(stats) = &self.neural_stats {
                        ui.label(format!("Throughput: {:.2} Mbps", stats.throughput_mbps));
                        ui.label(format!("Compression: {:.2}x", stats.compression_ratio));
                        ui.label(format!("Spike Count: {}", stats.spike_count));
                        ui.label(format!("Latency: {:?}", stats.duration));
                    } else {
                        ui.label("No neural traffic detected");
                    }
                });

                ui.group(|ui| {
                    ui.label(egui::RichText::new("Domain-Specific ASIC").strong());
                    egui::ComboBox::from_label("Modality")
                        .selected_text(format!("{:?}", self.selected_domain))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.selected_domain, spine_agentic::ProtocolDomain::BulkData, "BulkData");
                            ui.selectable_value(&mut self.selected_domain, spine_agentic::ProtocolDomain::Text, "Text");
                            ui.selectable_value(&mut self.selected_domain, spine_agentic::ProtocolDomain::Image, "Image");
                            ui.selectable_value(&mut self.selected_domain, spine_agentic::ProtocolDomain::Audio, "Audio");
                            ui.selectable_value(&mut self.selected_domain, spine_agentic::ProtocolDomain::Video, "Video");
                            ui.selectable_value(&mut self.selected_domain, spine_agentic::ProtocolDomain::StructuredData, "StructuredData");
                            ui.selectable_value(&mut self.selected_domain, spine_agentic::ProtocolDomain::Code, "Code");
                            ui.selectable_value(&mut self.selected_domain, spine_agentic::ProtocolDomain::NeuralWeights, "NeuralWeights");
                        });

                    if ui.button("Run Neural Benchmark").clicked() {
                        let agent_clone = self.agent.clone();
                        let tx = self.event_tx.clone();
                        let domain = self.selected_domain;
                        self.rt.spawn(async move {
                            let mut lock = agent_clone.lock().await;
                            let agent = match lock.as_mut() {
                                Some(a) => a,
                                None => return,
                            };
                            
                            let data = vec![0u8; 1024 * 1024]; // 1MB
                            if let Ok(stats) = agent.transmit_neural(&data, domain).await {
                                let _ = tx.send(BrowserEvent::NeuralStatsUpdated(stats)).await;
                            }
                        });
                    }
                });
            });
        }

        if self.show_communication {
            egui::SidePanel::left("comms_panel").resizable(true).show(ctx, |ui| {
                ui.heading("Agent Communication");
                ui.separator();

                ui.group(|ui| {
                    ui.label(egui::RichText::new("Send Speech Act").strong());
                    ui.horizontal(|ui| {
                        ui.label("Target ID:");
                        ui.text_edit_singleline(&mut self.comms_target_id);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Performative:");
                        egui::ComboBox::from_label("")
                            .selected_text(&self.comms_performative)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.comms_performative, "Inform".to_string(), "Inform");
                                ui.selectable_value(&mut self.comms_performative, "Request".to_string(), "Request");
                                ui.selectable_value(&mut self.comms_performative, "Propose".to_string(), "Propose");
                                ui.selectable_value(&mut self.comms_performative, "Accept".to_string(), "Accept");
                                ui.selectable_value(&mut self.comms_performative, "Reject".to_string(), "Reject");
                                ui.selectable_value(&mut self.comms_performative, "CallForProposal".to_string(), "CallForProposal");
                            });
                    });
                    ui.label("Content:");
                    ui.add(egui::TextEdit::multiline(&mut self.comms_content).desired_rows(2));
                    if ui.button("Send Act").clicked() {
                        self.send_speech_act();
                    }
                });

                ui.separator();
                ui.group(|ui| {
                    ui.label(egui::RichText::new("Speech Acts (FIPA)").strong());
                    if self.speech_acts.is_empty() {
                        ui.label("No active conversations");
                    } else {
                        for act in &self.speech_acts {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(format!("{:?}", act.speech_act)).color(egui::Color32::GOLD));
                                    ui.label(format!("from: {}", act.sender.to_string().get(0..8).unwrap_or("...")));
                                });
                                match &act.speech_act {
                                    spine_agentic::SpeechAct::Inform { content } => {
                                        ui.label(format!("Content: {}", content));
                                    }
                                    spine_agentic::SpeechAct::Request { action, .. } => {
                                        ui.label(format!("Request: {}", action));
                                    }
                                    _ => {
                                        ui.label("Other speech act");
                                    }
                                }
                            });
                        }
                    }
                });

                ui.separator();
                ui.group(|ui| {
                    ui.label(egui::RichText::new("Contract Net Protocol").strong());
                    ui.label("Manager: Active");
                    ui.label("Bids received: 0");
                });
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.elements.is_empty() {
                    ui.label(&self.content);
                } else {
                    self.render_elements(ui, None);
                }
            });
        });

        // Request a repaint to keep the UI responsive
        ctx.request_repaint();
    }
}

fn main() -> eframe::Result<()> {
    env_logger::init();
    
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "SPINE Browser",
        native_options,
        Box::new(|cc| Ok(Box::new(SpineBrowser::new(cc)))),
    )
}
