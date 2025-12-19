use hyperlight_protocol::{Message, ProtocolHandler, BrowserCommand, Request, Response, HyperlightBinary};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use hyperlight_parser::UnifiedRepresentation;
use hyperlight_cluster::ClusterClient;
use std::net::SocketAddr;

// Re-export the compiler for convenience
pub use hyperlight_compiler::Compiler;

pub struct AgentClient {
    /// Public handler for advanced protocol operations (Chameleon, Speculation, etc.)
    pub handler: ProtocolHandler,
    request_counter: u64,
    response_txs: std::collections::HashMap<String, mpsc::Sender<Response>>,
    latent_tx: Option<mpsc::Sender<Vec<f32>>>,
    event_tx: Option<mpsc::Sender<hyperlight_protocol::Event>>,
}

impl AgentClient {
    pub async fn connect(addr: &str) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let handler = ProtocolHandler::new(stream);
        Ok(Self { 
            handler, 
            request_counter: 0,
            response_txs: std::collections::HashMap::new(),
            latent_tx: None,
            event_tx: None,
        })
    }

    /// Start listening for incoming messages in the background.
    /// This is required for latent streaming and events.
    pub async fn start_listener(&mut self) -> (mpsc::Receiver<Vec<f32>>, mpsc::Receiver<hyperlight_protocol::Event>) {
        let (latent_tx, latent_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(100);
        
        self.latent_tx = Some(latent_tx);
        self.event_tx = Some(event_tx);
        
        (latent_rx, event_rx)
    }

    async fn send_request(&mut self, command: BrowserCommand) -> anyhow::Result<Response> {
        self.request_counter += 1;
        let id = self.request_counter.to_string();
        let req = Request { id: id.clone(), command };
        self.handler.send_message(&Message::Request(req)).await?;
        
        // If we have a listener, we should wait for the response via a channel.
        // For now, we'll keep the simple loop if no listener is active.
        loop {
            match self.handler.receive_message().await? {
                Message::Response(res) if res.id == id => return Ok(res),
                Message::LatentMessage(vec) => {
                    if let Some(tx) = &self.latent_tx {
                        let _ = tx.send(vec.components).await;
                    }
                }
                Message::Event(ev) => {
                    if let Some(tx) = &self.event_tx {
                        let _ = tx.send(ev).await;
                    }
                }
                _ => continue,
            }
        }
    }

    pub async fn navigate(&mut self, url: &str) -> anyhow::Result<()> {
        let res = self.send_request(BrowserCommand::Navigate { url: url.to_string() }).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn get_ur(&mut self) -> anyhow::Result<UnifiedRepresentation> {
        let res = self.send_request(BrowserCommand::GetUR).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let ur: UnifiedRepresentation = serde_json::from_value(res.result.unwrap())?;
        Ok(ur)
    }

    pub async fn get_raw_html(&mut self) -> anyhow::Result<String> {
        let res = self.send_request(BrowserCommand::GetRawHTML).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let html = res.result.unwrap()["html"].as_str().unwrap_or_default().to_string();
        Ok(html)
    }

    pub async fn click(&mut self, element_id: &str) -> anyhow::Result<()> {
        let res = self.send_request(BrowserCommand::Click { element_id: element_id.to_string() }).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn type_text(&mut self, element_id: &str, text: &str) -> anyhow::Result<()> {
        let res = self.send_request(BrowserCommand::Type { 
            element_id: element_id.to_string(), 
            text: text.to_string() 
        }).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn execute_binary(&mut self, binary: HyperlightBinary) -> anyhow::Result<serde_json::Value> {
        let res = self.send_request(BrowserCommand::ExecuteBinary(binary)).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(res.result.unwrap_or(serde_json::Value::Null))
    }

    pub async fn get_latent_ur(&mut self, dimensions: usize) -> anyhow::Result<Vec<f32>> {
        let res = self.send_request(BrowserCommand::GetLatentUR { dimensions }).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let latent: Vec<f32> = serde_json::from_value(res.result.unwrap()["latent_vector"].clone())?;
        Ok(latent)
    }

    /// Subscribe to latent streams from the core
    pub async fn subscribe_latent(&mut self) -> anyhow::Result<mpsc::Receiver<Vec<f32>>> {
        if let Some(tx) = &self.latent_tx {
            // This is a bit tricky since we can only have one receiver for the channel.
            // In a real implementation, we'd use a broadcast channel or multiple subscribers.
            anyhow::bail!("Already subscribed to latent stream");
        }
        
        let (tx, rx) = mpsc::channel(100);
        self.latent_tx = Some(tx);
        Ok(rx)
    }

    pub async fn morph(&mut self) -> anyhow::Result<()> {
        let res = self.send_request(BrowserCommand::Morph).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    /// Get statistics about speculative decoding performance
    pub fn get_speculation_stats(&self) -> &hyperlight_protocol::SpeculationStats {
        self.handler.get_speculation_stats()
    }

    pub async fn search(&mut self, query: &str) -> anyhow::Result<serde_json::Value> {
        let res = self.send_request(BrowserCommand::Search { query: query.to_string() }).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(res.result.unwrap())
    }

    pub async fn transfer_session(&mut self, target_node_id: uuid::Uuid) -> anyhow::Result<()> {
        let res = self.send_request(BrowserCommand::TransferSession { target_node_id }).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }
}
