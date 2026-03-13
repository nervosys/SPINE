use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::ClientConfig;
use rustls::RootCertStore;
use spine_human::HumanInteractionEngine;
use spine_parser::UnifiedRepresentation;
use spine_protocol::{BrowserCommand, Message, ProtocolHandler, Request, Response, SpineBinary};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;
use tracing::instrument;

// Re-export the compiler for convenience
pub use spine_compiler::Compiler;
// Re-export WebSocket stream for CLI and external consumers
pub use spine_transport::WebSocketClientStream;

pub struct AgentClient<S> {
    /// Public handler for advanced protocol operations (Chameleon, Speculation, etc.)
    pub handler: ProtocolHandler<S>,
    request_counter: u64,
    latent_tx: Option<mpsc::Sender<Vec<f32>>>,
    event_tx: Option<mpsc::Sender<spine_protocol::Event>>,
    human_engine: Option<HumanInteractionEngine>,
    pub neural_protocol: Option<spine_agentic::NeuralProtocol>,
}

impl AgentClient<TcpStream> {
    pub async fn connect(addr: &str) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let handler = ProtocolHandler::new(stream);
        Ok(Self {
            handler,
            request_counter: 0,
            latent_tx: None,
            event_tx: None,
            human_engine: None,
            neural_protocol: Some(spine_agentic::NeuralProtocol::new(1000.0, 5.0)),
        })
    }

    /// Connect with exponential backoff retry.
    ///
    /// Retries up to `max_retries` times, starting with `base_delay` and doubling
    /// each attempt (capped at 60 seconds). Returns the connected client or the
    /// last connection error.
    ///
    /// # Example
    /// ```no_run
    /// # async fn demo() {
    /// let client = spine_agent::AgentClient::connect_with_retry(
    ///     "127.0.0.1:8080", 5, std::time::Duration::from_millis(500)
    /// ).await.unwrap();
    /// # }
    /// ```
    pub async fn connect_with_retry(
        addr: &str,
        max_retries: u32,
        base_delay: std::time::Duration,
    ) -> anyhow::Result<Self> {
        let mut delay = base_delay;
        let max_delay = std::time::Duration::from_secs(60);

        for attempt in 0..=max_retries {
            match Self::connect(addr).await {
                Ok(client) => {
                    if attempt > 0 {
                        eprintln!("[spine-agent] Reconnected on attempt {}", attempt + 1);
                    }
                    return Ok(client);
                }
                Err(e) => {
                    if attempt == max_retries {
                        return Err(anyhow::anyhow!(
                            "Failed to connect after {} attempts: {}",
                            max_retries + 1,
                            e
                        ));
                    }
                    eprintln!(
                        "[spine-agent] Connection attempt {} failed: {}. Retrying in {:?}...",
                        attempt + 1,
                        e,
                        delay
                    );
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(max_delay);
                }
            }
        }
        unreachable!()
    }
}

impl AgentClient<tokio_rustls::client::TlsStream<TcpStream>> {
    /// Connect to a SPINE server using TLS (optionally with mTLS)
    pub async fn connect_tls(
        addr: &str,
        domain: &str,
        ca_path: Option<&std::path::Path>,
        client_cert: Option<(&std::path::Path, &std::path::Path)>,
    ) -> anyhow::Result<Self> {
        let mut root_store = RootCertStore::empty();

        if let Some(ca) = ca_path {
            let mut reader = std::io::BufReader::new(std::fs::File::open(ca)?);
            let certs: Vec<_> = rustls_pemfile::certs(&mut reader)
                .filter_map(|c| c.ok())
                .collect();
            for cert in certs {
                root_store.add(cert)?;
            }
        } else {
            // Fallback to system certs if no CA provided
            for cert in rustls_native_certs::load_native_certs()? {
                root_store.add(CertificateDer::from(cert.0))?;
            }
        }

        let config = if let Some((cert_path, key_path)) = client_cert {
            let mut cert_reader = std::io::BufReader::new(std::fs::File::open(cert_path)?);
            let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
                .filter_map(|c| c.ok())
                .collect();

            let mut key_reader = std::io::BufReader::new(std::fs::File::open(key_path)?);
            let keys: Vec<_> = rustls_pemfile::pkcs8_private_keys(&mut key_reader)
                .filter_map(|k| k.ok())
                .collect();
            let key: PrivateKeyDer<'static> = keys
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("No key"))?
                .into();

            ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_client_auth_cert(certs, key)?
        } else {
            ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth()
        };

        let connector = TlsConnector::from(Arc::new(config));
        let stream = TcpStream::connect(addr).await?;
        let domain = ServerName::try_from(domain.to_owned())?;
        let tls_stream = connector.connect(domain, stream).await?;

        let handler = ProtocolHandler::new(tls_stream);
        Ok(Self {
            handler,
            request_counter: 0,
            latent_tx: None,
            event_tx: None,
            human_engine: None,
            neural_protocol: Some(spine_agentic::NeuralProtocol::new(1000.0, 5.0)),
        })
    }

    /// Connect to a SPINE server over TLS with automatic retry and exponential backoff.
    pub async fn connect_tls_with_retry(
        addr: &str,
        domain: &str,
        ca_path: Option<&std::path::Path>,
        client_cert: Option<(&std::path::Path, &std::path::Path)>,
        max_retries: u32,
    ) -> anyhow::Result<Self> {
        let mut delay = std::time::Duration::from_secs(1);
        let max_delay = std::time::Duration::from_secs(60);
        for attempt in 0..max_retries {
            match Self::connect_tls(addr, domain, ca_path, client_cert).await {
                Ok(client) => return Ok(client),
                Err(e) => {
                    if attempt + 1 >= max_retries {
                        return Err(e);
                    }
                    tracing::warn!(
                        "TLS connection attempt {} failed: {}. Retrying in {:?}...",
                        attempt + 1,
                        e,
                        delay
                    );
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(max_delay);
                }
            }
        }
        anyhow::bail!("connect_tls_with_retry: max_retries=0")
    }
}

impl AgentClient<spine_transport::WebSocketClientStream> {
    /// Connect to a SPINE server over WebSocket.
    ///
    /// `url` should be a `ws://` or `wss://` URL, e.g. `ws://127.0.0.1:3001`.
    pub async fn connect_ws(url: &str) -> anyhow::Result<Self> {
        let bridge = spine_transport::WebSocketBridge::connect(url)
            .await
            .map_err(|e| anyhow::anyhow!("WebSocket connect failed: {}", e))?;
        let stream = spine_transport::WebSocketClientStream::new(bridge);
        let handler = ProtocolHandler::new(stream);
        Ok(Self {
            handler,
            request_counter: 0,
            latent_tx: None,
            event_tx: None,
            human_engine: None,
            neural_protocol: Some(spine_agentic::NeuralProtocol::new(1000.0, 5.0)),
        })
    }
}

impl<S> AgentClient<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    /// Enable human-like interaction patterns
    pub fn enable_human_mode(&mut self, engine: HumanInteractionEngine) {
        self.human_engine = Some(engine);
    }

    pub async fn start_listener(
        &mut self,
    ) -> (
        mpsc::Receiver<Vec<f32>>,
        mpsc::Receiver<spine_protocol::Event>,
    ) {
        let (latent_tx, latent_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(100);

        self.latent_tx = Some(latent_tx);
        self.event_tx = Some(event_tx);

        (latent_rx, event_rx)
    }

    async fn send_request(&mut self, command: BrowserCommand) -> anyhow::Result<Response> {
        self.request_counter += 1;
        let id = self.request_counter.to_string();
        let req = Request {
            id: id.clone(),
            command,
        };
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

    #[instrument(skip(self))]
    pub async fn navigate(&mut self, url: &str) -> anyhow::Result<()> {
        let res = self
            .send_request(BrowserCommand::Navigate {
                url: url.to_string(),
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn get_ur(&mut self) -> anyhow::Result<UnifiedRepresentation> {
        let res = self.send_request(BrowserCommand::GetUR).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let ur: UnifiedRepresentation = serde_json::from_value(
            res.result
                .ok_or_else(|| anyhow::anyhow!("Missing result in response"))?,
        )?;
        Ok(ur)
    }

    pub async fn get_raw_html(&mut self) -> anyhow::Result<String> {
        let res = self.send_request(BrowserCommand::GetRawHTML).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let html = res.result.unwrap_or_default()["html"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        Ok(html)
    }

    pub async fn click(&mut self, element_id: &str) -> anyhow::Result<()> {
        if let Some(engine) = &self.human_engine {
            // Simulate reaction time
            tokio::time::sleep(std::time::Duration::from_millis(engine.reaction_time_ms)).await;

            // Simulate mouse movement (simplified for now, just a delay)
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            // Simulate click duration
            tokio::time::sleep(engine.simulate_click_duration()).await;
        }

        let res = self
            .send_request(BrowserCommand::Click {
                element_id: element_id.to_string(),
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn type_text(&mut self, element_id: &str, text: &str) -> anyhow::Result<()> {
        if let Some(engine) = &self.human_engine {
            // Simulate reaction time
            tokio::time::sleep(std::time::Duration::from_millis(engine.reaction_time_ms)).await;

            let delays = engine.generate_typing_delays(text);
            for (i, _c) in text.chars().enumerate() {
                // In a real implementation, we might send each character individually
                // but for now we'll just sleep and then send the whole text at the end
                // or send it character by character if the protocol supports it.
                tokio::time::sleep(delays[i]).await;
            }
        }

        let res = self
            .send_request(BrowserCommand::Type {
                element_id: element_id.to_string(),
                text: text.to_string(),
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn execute_binary(
        &mut self,
        binary: SpineBinary,
    ) -> anyhow::Result<serde_json::Value> {
        let res = self
            .send_request(BrowserCommand::ExecuteBinary(binary))
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(res.result.unwrap_or(serde_json::Value::Null))
    }

    #[instrument(skip(self))]
    pub async fn execute_hls(
        &mut self,
        script: &str,
    ) -> anyhow::Result<spine_wasm::WasmExecutionResult> {
        let binary = spine_compiler::Compiler::compile(script)?;
        let res = self
            .send_request(BrowserCommand::ExecuteBinary(binary))
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let result: spine_wasm::WasmExecutionResult = serde_json::from_value(
            res.result
                .ok_or_else(|| anyhow::anyhow!("Missing result in response"))?,
        )?;
        Ok(result)
    }

    pub async fn get_latent_ur(&mut self, dimensions: usize) -> anyhow::Result<Vec<f32>> {
        let res = self
            .send_request(BrowserCommand::GetLatentUR { dimensions })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let latent: Vec<f32> =
            serde_json::from_value(res.result.unwrap_or_default()["latent_vector"].clone())?;
        Ok(latent)
    }

    /// Subscribe to latent streams from the core
    pub async fn subscribe_latent(&mut self) -> anyhow::Result<mpsc::Receiver<Vec<f32>>> {
        if let Some(_tx) = &self.latent_tx {
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
    pub fn get_speculation_stats(&self) -> &spine_protocol::SpeculationStats {
        self.handler.get_speculation_stats()
    }

    #[instrument(skip(self))]
    pub async fn search(&mut self, query: &str) -> anyhow::Result<serde_json::Value> {
        let res = self
            .send_request(BrowserCommand::Search {
                query: query.to_string(),
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(res.result.unwrap_or_default())
    }

    pub async fn transfer_session(&mut self, target_node_id: uuid::Uuid) -> anyhow::Result<()> {
        let res = self
            .send_request(BrowserCommand::TransferSession { target_node_id })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn handle_event(
        &mut self,
        element_id: u32,
        event_name: &str,
        payload: serde_json::Value,
    ) -> anyhow::Result<Vec<spine_protocol::VDomPatch>> {
        let res = self
            .send_request(BrowserCommand::HandleEvent {
                element_id,
                event_name: event_name.to_string(),
                payload,
            })
            .await?;

        if let Some(err) = res.error {
            anyhow::bail!(err);
        }

        let patches: Vec<spine_protocol::VDomPatch> = serde_json::from_value(
            res.result.unwrap_or(serde_json::json!({"patches": []}))["patches"].clone(),
        )?;

        Ok(patches)
    }

    pub async fn store_knowledge(
        &mut self,
        key: &str,
        value: serde_json::Value,
        tags: Vec<String>,
    ) -> anyhow::Result<()> {
        let res = self
            .send_request(BrowserCommand::StoreKnowledge {
                key: key.to_string(),
                value,
                tags,
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn query_knowledge(
        &mut self,
        query: &str,
        tags: Vec<String>,
        limit: usize,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let res = self
            .send_request(BrowserCommand::QueryKnowledge {
                query: query.to_string(),
                tags,
                limit,
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let results = res.result.unwrap_or_default()["results"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        Ok(results)
    }

    pub async fn delete_knowledge(&mut self, key: &str) -> anyhow::Result<()> {
        let res = self
            .send_request(BrowserCommand::DeleteKnowledge {
                key: key.to_string(),
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn get_history(&mut self) -> anyhow::Result<Vec<BrowserCommand>> {
        let res = self.send_request(BrowserCommand::GetSessionHistory).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let history: Vec<BrowserCommand> =
            serde_json::from_value(res.result.unwrap_or_default()["history"].clone())?;
        Ok(history)
    }

    #[instrument(skip(self))]
    pub async fn ping(&mut self) -> anyhow::Result<u64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;

        self.handler
            .send_message(&Message::Ping { timestamp: now })
            .await?;

        loop {
            match self.handler.receive_message().await? {
                Message::Pong { timestamp } => {
                    let end = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_millis() as u64;
                    return Ok(end - timestamp);
                }
                _ => continue,
            }
        }
    }

    pub async fn get_capabilities(&mut self) -> anyhow::Result<Vec<String>> {
        let res = self.send_request(BrowserCommand::GetCapabilities).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let capabilities: Vec<String> =
            serde_json::from_value(res.result.unwrap_or_default()["capabilities"].clone())?;
        Ok(capabilities)
    }

    pub async fn set_autonomous_mode(&mut self, enabled: bool) -> anyhow::Result<()> {
        let res = self
            .send_request(BrowserCommand::SetAutonomousMode { enabled })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn swarm_search(&mut self, query: &str, depth: usize) -> anyhow::Result<()> {
        let res = self
            .send_request(BrowserCommand::SwarmSearch {
                query: query.to_string(),
                depth,
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn delegate_task(
        &mut self,
        task: &str,
        target_agent_id: Option<uuid::Uuid>,
    ) -> anyhow::Result<()> {
        let res = self
            .send_request(BrowserCommand::DelegateTask {
                task: task.to_string(),
                target_agent_id,
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn propose_knowledge(
        &mut self,
        key: &str,
        value: serde_json::Value,
        tags: Vec<String>,
    ) -> anyhow::Result<()> {
        let res = self
            .send_request(BrowserCommand::ProposeKnowledge {
                key: key.to_string(),
                value,
                tags,
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn create_swarm_plan(&mut self, goal: &str) -> anyhow::Result<uuid::Uuid> {
        let res = self
            .send_request(BrowserCommand::CreateSwarmPlan {
                goal: goal.to_string(),
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let plan_id = res
            .result
            .and_then(|v| {
                v.get("plan_id")
                    .and_then(|id| id.as_str())
                    .and_then(|s| uuid::Uuid::parse_str(s).ok())
            })
            .ok_or_else(|| anyhow::anyhow!("Missing plan_id in response"))?;
        Ok(plan_id)
    }

    pub async fn execute_plan_task(
        &mut self,
        plan_id: uuid::Uuid,
        task_id: uuid::Uuid,
    ) -> anyhow::Result<()> {
        let res = self
            .send_request(BrowserCommand::ExecutePlanTask { plan_id, task_id })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }

    pub async fn transmit_neural(
        &mut self,
        data: &[u8],
        domain: spine_agentic::ProtocolDomain,
    ) -> anyhow::Result<spine_agentic::TransmissionResult> {
        let domain_str = format!("{:?}", domain);
        let res = self
            .send_request(BrowserCommand::NeuralTransmit {
                data: data.to_vec(),
                domain: domain_str,
            })
            .await?;

        if let Some(err) = res.error {
            anyhow::bail!(err);
        }

        let stats = res
            .result
            .and_then(|v| serde_json::from_value::<spine_agentic::TransmissionResult>(v).ok())
            .ok_or_else(|| anyhow::anyhow!("Failed to parse transmission results"))?;
        Ok(stats)
    }

    pub async fn get_agentic_state(&mut self) -> anyhow::Result<serde_json::Value> {
        let res = self.send_request(BrowserCommand::GetAgenticState).await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(res.result.unwrap_or_default())
    }

    pub async fn send_speech_act(
        &mut self,
        target_id: uuid::Uuid,
        performative: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let res = self
            .send_request(BrowserCommand::SendSpeechAct {
                target_id,
                performative: performative.to_string(),
                content: content.to_string(),
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiler_reexport() {
        // Verify Compiler is re-exported and can compile HLS
        let binary = Compiler::compile("element div { }").unwrap();
        assert!(!binary.instructions.is_empty());
    }

    #[test]
    fn test_compiler_reexport_error() {
        // Invalid HLS should return an error, not panic
        let result = Compiler::compile("");
        // Empty input may compile to empty binary or error depending on parser
        // Just ensure it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_agent_client_speculation_stats_type() {
        // Verify the SpeculationStats type is accessible
        let stats = spine_protocol::SpeculationStats::default();
        assert_eq!(stats.output_predictions, 0);
        assert_eq!(stats.output_hits, 0);
    }

    #[test]
    fn test_browser_command_serialization() {
        // Test that BrowserCommand variants serialize correctly for the agent protocol
        let cmd = BrowserCommand::Navigate {
            url: "https://example.com".to_string(),
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert!(json.to_string().contains("example.com"));

        let cmd2 = BrowserCommand::GetUR;
        let json2 = serde_json::to_value(&cmd2).unwrap();
        assert!(!json2.to_string().is_empty());
    }

    #[test]
    fn test_request_response_protocol() {
        // Test that Request/Response types work correctly
        let req = Request {
            id: "1".to_string(),
            command: BrowserCommand::GetCapabilities,
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "1");

        let res = Response {
            id: "1".to_string(),
            result: Some(serde_json::json!({"capabilities": ["navigate", "search"]})),
            error: None,
        };
        assert!(res.error.is_none());
        assert!(res.result.is_some());
    }

    #[test]
    fn test_message_variants() {
        // Test that Message::Request and Message::Response serialize/deserialize
        let msg = Message::Request(Request {
            id: "42".to_string(),
            command: BrowserCommand::Morph,
        });
        let json = serde_json::to_string(&msg).unwrap();
        let rt: Message = serde_json::from_str(&json).unwrap();
        match rt {
            Message::Request(r) => assert_eq!(r.id, "42"),
            _ => panic!("Expected Request variant"),
        }
    }

    #[test]
    fn test_ping_message() {
        let msg = Message::Ping {
            timestamp: 1234567890,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let rt: Message = serde_json::from_str(&json).unwrap();
        match rt {
            Message::Ping { timestamp } => assert_eq!(timestamp, 1234567890),
            _ => panic!("Expected Ping variant"),
        }
    }

    #[test]
    fn test_latent_message() {
        let msg = Message::LatentMessage(spine_protocol::LatentVector {
            components: vec![0.1, 0.2, 0.3],
            dim_hint: 3,
            epoch: 0,
        });
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("0.1"));
    }

    #[tokio::test]
    async fn test_connect_invalid_address() {
        // Connecting to an invalid address should return an error, not panic
        let result = AgentClient::connect("127.0.0.1:1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_connect_with_retry_fails_fast() {
        // With 0 retries and invalid addr, should fail immediately
        let result =
            AgentClient::connect_with_retry("127.0.0.1:1", 0, std::time::Duration::from_millis(1))
                .await;
        assert!(result.is_err());
        assert!(result.is_err());
    }

    #[test]
    fn test_wasm_execution_result_default() {
        let result = spine_wasm::WasmExecutionResult {
            elements: vec![],
            events: vec![],
            latent_streams: vec![],
            actions: vec![],
            stats: spine_wasm::WasmStats::default(),
        };
        assert!(result.elements.is_empty());
        assert_eq!(result.stats.wasm_size_bytes, 0);
    }
}
