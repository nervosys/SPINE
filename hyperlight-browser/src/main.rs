use eframe::egui;
use hyperlight_agent::AgentClient;
use hyperlight_human::HumanTranspiler;
use hyperlight_wasm::WasmElement;
use hyperlight_protocol::VDomPatch;
use hyperlight_cluster::ClusterClient;
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
    Error(String),
}

struct HyperlightBrowser {
    url: String,
    search_query: String,
    content: String,
    hls_preview: String,
    status: String,
    elements: Vec<WasmElement>,
    latent_vector: Vec<f32>,
    agent: Arc<Mutex<Option<AgentClient<tokio::net::TcpStream>>>>,
    cluster_client: ClusterClient,
    rt: Runtime,
    human_mode: bool,
    history: Vec<String>,
    show_hls: bool,
    show_latent: bool,
    event_tx: mpsc::Sender<BrowserEvent>,
    event_rx: mpsc::Receiver<BrowserEvent>,
}

impl HyperlightBrowser {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            url: "https://example.com".to_string(),
            search_query: String::new(),
            content: "Welcome to Hyperlight Browser".to_string(),
            hls_preview: String::new(),
            status: "Disconnected".to_string(),
            elements: Vec::new(),
            latent_vector: Vec::new(),
            agent: Arc::new(Mutex::new(None)),
            cluster_client: ClusterClient::new(vec!["127.0.0.1:8080".parse().unwrap()]),
            rt: Runtime::new().unwrap(),
            human_mode: true,
            history: Vec::new(),
            show_hls: true,
            show_latent: false,
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
                }
                Err(e) => {
                    let _ = tx.send(BrowserEvent::Error(format!("Search failed: {}", e))).await;
                }
            }
        });
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
                        let element_id = elem.id.to_string();
                        let tx = self.event_tx.clone();
                        
                        self.rt.spawn(async move {
                            let mut lock = agent_clone.lock().await;
                            let agent: &mut AgentClient<tokio::net::TcpStream> = match lock.as_mut() {
                                Some(a) => a,
                                None => return,
                            };
                            
                            let _ = tx.send(BrowserEvent::StatusChanged(format!("Clicking {}...", element_id))).await;
                            match agent.click(&element_id).await {
                                Ok(_) => {
                                    let _ = tx.send(BrowserEvent::StatusChanged("Action successful".to_string())).await;
                                }
                                Err(e) => {
                                    let _ = tx.send(BrowserEvent::Error(format!("Click failed: {}", e))).await;
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

impl eframe::App for HyperlightBrowser {
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
                BrowserEvent::Error(e) => {
                    self.status = format!("Error: {}", e);
                    self.content = format!("Error: {}", e);
                }
            }
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Hyperlight Browser");
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

                ui.separator();
                ui.label("Search:");
                let res = ui.text_edit_singleline(&mut self.search_query);
                if res.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.search();
                }
                if ui.button("🔍").clicked() {
                    self.search();
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
                ui.checkbox(&mut self.show_hls, "Show HLS");
                ui.checkbox(&mut self.show_latent, "Show Latent");
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
            });
        });

        if self.show_hls && self.human_mode {
            egui::SidePanel::right("hls_panel").resizable(true).show(ctx, |ui| {
                ui.heading("HLS Preview");
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
        "Hyperlight Browser",
        native_options,
        Box::new(|cc| Ok(Box::new(HyperlightBrowser::new(cc)))),
    )
}
