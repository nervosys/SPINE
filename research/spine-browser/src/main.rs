// Allow dead code for browser GUI features
#![allow(dead_code)]

use eframe::egui;
use spine_agent::AgentClient;
use spine_cluster::ClusterClient;
use spine_human::HumanTranspiler;
use spine_protocol::VDomPatch;
use spine_wasm::WasmElement;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, Mutex};

enum BrowserEvent {
    StatusChanged(String),
    ContentUpdated(String),
    HlsUpdated(String),
    ElementsUpdated(Vec<WasmElement>),
    PatchesApplied(Vec<VDomPatch>),
    KnowledgeUpdated(Vec<serde_json::Value>),
    CapabilitiesUpdated(Vec<String>),
    ReasoningUpdated(Vec<spine_human::AgentAction>),
    Error(String),
}

struct SpineBrowser {
    url: String,
    hls_input: String,
    content: String,
    hls_preview: String,
    status: String,
    elements: Vec<WasmElement>,
    knowledge_base: Vec<serde_json::Value>,
    current_capabilities: Vec<String>,
    suggested_actions: Vec<spine_human::AgentAction>,
    new_memory_key: String,
    new_memory_value: String,
    agent: Arc<Mutex<Option<AgentClient<tokio::net::TcpStream>>>>,
    cluster_client: ClusterClient,
    rt: Runtime,
    human_mode: bool,
    history: Vec<String>,
    show_hls: bool,
    show_knowledge: bool,
    show_reasoning: bool,
    event_tx: mpsc::Sender<BrowserEvent>,
    event_rx: mpsc::Receiver<BrowserEvent>,
}

impl SpineBrowser {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            url: "https://example.com".to_string(),
            hls_input: "on_mount -> {\n  print(\"Hello from HLS!\");\n}".to_string(),
            content: "Welcome to SPINE Browser".to_string(),
            hls_preview: String::new(),
            status: "Disconnected".to_string(),
            elements: Vec::new(),
            knowledge_base: Vec::new(),
            current_capabilities: Vec::new(),
            suggested_actions: Vec::new(),
            new_memory_key: String::new(),
            new_memory_value: String::new(),
            agent: Arc::new(Mutex::new(None)),
            cluster_client: ClusterClient::new(vec!["127.0.0.1:8080".parse().unwrap()]),
            rt: Runtime::new().unwrap(),
            human_mode: true,
            history: Vec::new(),
            show_hls: true,
            show_knowledge: false,
            show_reasoning: true,
            event_tx: tx,
            event_rx: rx,
        }
    }

    fn connect(&mut self) {
        let agent_clone = self.agent.clone();
        let tx = self.event_tx.clone();
        let addr = self
            .cluster_client
            .get_connection(None)
            .unwrap_or_else(|| "127.0.0.1:8080".parse().unwrap());

        self.rt.spawn(async move {
            let _ = tx
                .send(BrowserEvent::StatusChanged(format!(
                    "Connecting to {}...",
                    addr
                )))
                .await;
            match AgentClient::connect(&addr.to_string()).await {
                Ok(client) => {
                    let mut lock = agent_clone.lock().await;
                    *lock = Some(client);
                    let _ = tx
                        .send(BrowserEvent::StatusChanged("Connected".to_string()))
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(BrowserEvent::Error(format!("Connection failed: {}", e)))
                        .await;
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

            let _ = tx
                .send(BrowserEvent::StatusChanged(format!(
                    "Navigating to {}...",
                    url
                )))
                .await;

            match agent.navigate(&url).await {
                Ok(_) => {
                    if human_mode {
                        match agent.get_raw_html().await {
                            Ok(html) => {
                                match HumanTranspiler::transpile(&html, "", "") {
                                    Ok(bin) => {
                                        let _ = tx
                                            .send(BrowserEvent::HlsUpdated(format!(
                                                "// Transpiled HLS for {}\n// Instructions: {}\n",
                                                url,
                                                bin.instructions.len()
                                            )))
                                            .await;
                                        let _ = tx
                                            .send(BrowserEvent::ContentUpdated(
                                                "Transpilation successful. Executing binary..."
                                                    .to_string(),
                                            ))
                                            .await;

                                        if let Ok(res) = agent.execute_binary(bin).await {
                                            let _ = tx
                                                .send(BrowserEvent::ContentUpdated(format!(
                                                    "Execution Result: {}",
                                                    res
                                                )))
                                                .await;
                                            if let Some(elements_val) = res.get("elements") {
                                                if let Ok(elements) =
                                                    serde_json::from_value::<Vec<WasmElement>>(
                                                        elements_val.clone(),
                                                    )
                                                {
                                                    let _ = tx
                                                        .send(BrowserEvent::ElementsUpdated(
                                                            elements,
                                                        ))
                                                        .await;
                                                }
                                            }
                                            if let Some(patches_val) = res.get("patches") {
                                                if let Ok(patches) =
                                                    serde_json::from_value::<Vec<VDomPatch>>(
                                                        patches_val.clone(),
                                                    )
                                                {
                                                    if !patches.is_empty() {
                                                        let _ = tx
                                                            .send(BrowserEvent::PatchesApplied(
                                                                patches,
                                                            ))
                                                            .await;
                                                    }
                                                }
                                            }
                                        }

                                        // Refresh knowledge and history
                                        if let Ok(kb) =
                                            agent.query_knowledge("*", vec![], 100).await
                                        {
                                            let _ =
                                                tx.send(BrowserEvent::KnowledgeUpdated(kb)).await;
                                        }
                                        if let Ok(caps) = agent.get_binary_capabilities().await {
                                            let _ = tx
                                                .send(BrowserEvent::CapabilitiesUpdated(caps))
                                                .await;
                                        }

                                        // Also trigger reasoning in human mode
                                        if let Ok(ur) = agent.get_ur().await {
                                            let engine = spine_human::ReasoningEngine::new();
                                            let actions = engine.suggest_actions(&ur);
                                            let _ = tx
                                                .send(BrowserEvent::ReasoningUpdated(actions))
                                                .await;
                                        }
                                    }
                                    Err(e) => {
                                        let _ = tx
                                            .send(BrowserEvent::Error(format!(
                                                "Transpilation failed: {}",
                                                e
                                            )))
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(BrowserEvent::Error(format!(
                                        "Failed to get raw HTML: {}",
                                        e
                                    )))
                                    .await;
                            }
                        }
                    } else {
                        match agent.get_ur().await {
                            Ok(ur) => {
                                let _ = tx
                                    .send(BrowserEvent::ContentUpdated(format!(
                                        "Unified Representation:\n{:#?}",
                                        ur
                                    )))
                                    .await;

                                // Trigger Reasoning Engine
                                let engine = spine_human::ReasoningEngine::new();
                                let actions = engine.suggest_actions(&ur);
                                let _ = tx.send(BrowserEvent::ReasoningUpdated(actions)).await;
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(BrowserEvent::Error(format!("Failed to get UR: {}", e)))
                                    .await;
                            }
                        }
                    }
                    let _ = tx
                        .send(BrowserEvent::StatusChanged("Ready".to_string()))
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(BrowserEvent::Error(format!("Navigation failed: {}", e)))
                        .await;
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
                VDomPatch::AppendChild {
                    parent_id,
                    child_id,
                } => {
                    if let Some(elem) = self.elements.iter_mut().find(|e| e.id == child_id) {
                        elem.parent_id = Some(parent_id);
                    }
                }
                VDomPatch::RemoveChild {
                    parent_id: _,
                    child_id,
                } => {
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
                    ui.heading(elem.attributes.get("text").cloned().unwrap_or_else(|| {
                        elem.attributes.get("content").cloned().unwrap_or_default()
                    }));
                }
                "Button" => {
                    let text = elem.attributes.get("text").cloned().unwrap_or_else(|| {
                        elem.attributes
                            .get("content")
                            .cloned()
                            .unwrap_or_else(|| "Button".to_string())
                    });
                    let _ = ui.button(text);
                }
                "Link" => {
                    let text = elem.attributes.get("text").cloned().unwrap_or_else(|| {
                        elem.attributes
                            .get("content")
                            .cloned()
                            .unwrap_or_else(|| "Link".to_string())
                    });
                    let href = elem.attributes.get("href").cloned().unwrap_or_default();
                    ui.hyperlink_to(text, href);
                }
                "text" | "Text" => {
                    let content = elem.attributes.get("content").cloned().unwrap_or_else(|| {
                        elem.attributes.get("text").cloned().unwrap_or_default()
                    });
                    ui.label(content);
                }
                "Image" => {
                    ui.label(format!(
                        "[Image: {}]",
                        elem.attributes.get("src").cloned().unwrap_or_default()
                    ));
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
                BrowserEvent::KnowledgeUpdated(kb) => self.knowledge_base = kb,
                BrowserEvent::CapabilitiesUpdated(caps) => self.current_capabilities = caps,
                BrowserEvent::ReasoningUpdated(actions) => self.suggested_actions = actions,
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
                                let _ = tx
                                    .send(BrowserEvent::StatusChanged(format!(
                                        "Latency: {}ms",
                                        latency
                                    )))
                                    .await;
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(BrowserEvent::Error(format!("Ping failed: {}", e)))
                                    .await;
                            }
                        }
                    });
                }

                ui.separator();
                ui.checkbox(&mut self.human_mode, "Human Mode");
                ui.checkbox(&mut self.show_hls, "HLS");
                ui.checkbox(&mut self.show_knowledge, "Memory");
                ui.checkbox(&mut self.show_reasoning, "Reasoning");
            });
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Status: {}", self.status));
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
            egui::SidePanel::right("hls_panel")
                .resizable(true)
                .show(ctx, |ui| {
                    ui.heading("HLS Script");
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.hls_input)
                                .font(egui::TextStyle::Monospace)
                                .code_editor(),
                        );
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

                            let _ = tx
                                .send(BrowserEvent::StatusChanged(
                                    "Compiling and executing HLS...".to_string(),
                                ))
                                .await;
                            match agent.execute_hls(&script).await {
                                Ok(result) => {
                                    let _ = tx
                                        .send(BrowserEvent::ElementsUpdated(result.elements))
                                        .await;
                                    let _ = tx
                                        .send(BrowserEvent::StatusChanged(
                                            "HLS executed successfully".to_string(),
                                        ))
                                        .await;

                                    // Handle autonomous actions
                                    for action in result.actions {
                                        match action {
                                            spine_wasm::WasmAction::Navigate(url) => {
                                                let _ = tx
                                                    .send(BrowserEvent::StatusChanged(format!(
                                                        "HLS Navigating to {}...",
                                                        url
                                                    )))
                                                    .await;
                                                // In a real browser, we'd trigger a navigation here
                                            }
                                            spine_wasm::WasmAction::Search(query) => {
                                                let _ = tx
                                                    .send(BrowserEvent::StatusChanged(format!(
                                                        "HLS Searching for {}...",
                                                        query
                                                    )))
                                                    .await;
                                            }
                                            spine_wasm::WasmAction::StoreKnowledge {
                                                key, ..
                                            } => {
                                                let _ = tx
                                                    .send(BrowserEvent::StatusChanged(format!(
                                                        "HLS Storing knowledge: {}...",
                                                        key
                                                    )))
                                                    .await;
                                            }
                                            spine_wasm::WasmAction::QueryKnowledge {
                                                query,
                                                ..
                                            } => {
                                                let _ = tx
                                                    .send(BrowserEvent::StatusChanged(format!(
                                                        "HLS Querying knowledge: {}...",
                                                        query
                                                    )))
                                                    .await;
                                            }
                                            spine_wasm::WasmAction::Reason(query) => {
                                                let _ = tx
                                                    .send(BrowserEvent::StatusChanged(format!(
                                                        "HLS Reasoning about {}...",
                                                        query
                                                    )))
                                                    .await;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = tx
                                        .send(BrowserEvent::Error(format!(
                                            "HLS execution failed: {}",
                                            e
                                        )))
                                        .await;
                                }
                            }
                        });
                    }

                    ui.separator();
                    ui.heading("HLS Preview (Compiled)");
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.hls_preview)
                                .font(egui::TextStyle::Monospace)
                                .code_editor(),
                        );
                    });
                });
        }

        if self.show_knowledge {
            egui::SidePanel::left("knowledge_panel")
                .resizable(true)
                .show(ctx, |ui| {
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

                                if agent
                                    .store_knowledge(
                                        &key,
                                        serde_json::json!(val),
                                        vec!["manual".to_string()],
                                    )
                                    .await
                                    .is_ok()
                                {
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
                                    ui.label(
                                        serde_json::to_string_pretty(item).unwrap_or_default(),
                                    );
                                });
                            });
                        }
                    });
                });
        }

        if self.show_reasoning {
            egui::SidePanel::right("reasoning_panel")
                .resizable(true)
                .show(ctx, |ui| {
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
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(format!("{:.0}%", action.confidence * 100.0));
                                        },
                                    );
                                });
                                ui.label(format!("Reason: {}", action.reasoning));
                                if let Some(target) = &action.target_id {
                                    ui.label(format!("Target ID: {}", target));
                                }
                                if ui.button("Execute").clicked() {
                                    // In a real implementation, this would trigger the action
                                    self.status = format!(
                                        "Executing suggested action: {}",
                                        action.action_type
                                    );
                                }
                            });
                        }
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
