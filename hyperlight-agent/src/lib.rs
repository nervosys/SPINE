use hyperlight_protocol::{Message, ProtocolHandler, BrowserCommand, Request, Response, HyperlightBinary};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use hyperlight_parser::UnifiedRepresentation;
use tokio::io::{AsyncRead, AsyncWrite};
use hyperlight_human::HumanInteractionEngine;
use std::sync::Arc;
use tokio_rustls::TlsConnector;
use rustls::{ClientConfig, RootCertStore, Certificate, PrivateKey};

// Re-export the compiler for convenience
pub use hyperlight_compiler::Compiler;

pub struct AgentClient<S> {
    /// Public handler for advanced protocol operations (Chameleon, Speculation, etc.)
    pub handler: ProtocolHandler<S>,
    request_counter: u64,
    latent_tx: Option<mpsc::Sender<Vec<f32>>>,
    event_tx: Option<mpsc::Sender<hyperlight_protocol::Event>>,
    human_engine: Option<HumanInteractionEngine>,
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
        })
    }
}

impl AgentClient<tokio_rustls::client::TlsStream<TcpStream>> {
    /// Connect to a Hyperlight server using TLS (optionally with mTLS)
    pub async fn connect_tls(
        addr: &str, 
        domain: &str,
        ca_path: Option<&std::path::Path>,
        client_cert: Option<(&std::path::Path, &std::path::Path)>
    ) -> anyhow::Result<Self> {
        let mut root_store = RootCertStore::empty();
        
        if let Some(ca) = ca_path {
            let mut reader = std::io::BufReader::new(std::fs::File::open(ca)?);
            let certs = rustls_pemfile::certs(&mut reader)?;
            for cert in certs {
                root_store.add(&Certificate(cert))?;
            }
        } else {
            // Fallback to system certs if no CA provided
            let certs = rustls_native_certs::load_native_certs()?;
            for cert in certs {
                root_store.add(&Certificate(cert.0))?;
            }
        }

        let config = if let Some((cert_path, key_path)) = client_cert {
            let mut cert_reader = std::io::BufReader::new(std::fs::File::open(cert_path)?);
            let certs = rustls_pemfile::certs(&mut cert_reader)?
                .into_iter()
                .map(Certificate)
                .collect();
            
            let mut key_reader = std::io::BufReader::new(std::fs::File::open(key_path)?);
            let keys = rustls_pemfile::pkcs8_private_keys(&mut key_reader)?;
            let key = PrivateKey(keys[0].clone());

            ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(root_store)
                .with_client_auth_cert(certs, key)?
        } else {
            ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(root_store)
                .with_no_client_auth()
        };

        let connector = TlsConnector::from(Arc::new(config));
        let stream = TcpStream::connect(addr).await?;
        let domain = rustls::ServerName::try_from(domain)?;
        let tls_stream = connector.connect(domain, stream).await?;
        
        let handler = ProtocolHandler::new(tls_stream);
        Ok(Self {
            handler,
            request_counter: 0,
            latent_tx: None,
            event_tx: None,
            human_engine: None,
        })
    }
}

impl<S> AgentClient<S> 
where 
    S: AsyncRead + AsyncWrite + Unpin + Send
{
    /// Enable human-like interaction patterns
    pub fn enable_human_mode(&mut self, engine: HumanInteractionEngine) {
        self.human_engine = Some(engine);
    }

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
        if let Some(engine) = &self.human_engine {
            // Simulate reaction time
            tokio::time::sleep(std::time::Duration::from_millis(engine.reaction_time_ms)).await;
            
            // Simulate mouse movement (simplified for now, just a delay)
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            
            // Simulate click duration
            tokio::time::sleep(engine.simulate_click_duration()).await;
        }

        let res = self.send_request(BrowserCommand::Click { element_id: element_id.to_string() }).await?;
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
            for (i, c) in text.chars().enumerate() {
                // In a real implementation, we might send each character individually
                // but for now we'll just sleep and then send the whole text at the end
                // or send it character by character if the protocol supports it.
                tokio::time::sleep(delays[i]).await;
            }
        }

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

    pub async fn handle_event(&mut self, element_id: u32, event_name: &str, payload: serde_json::Value) -> anyhow::Result<Vec<hyperlight_protocol::VDomPatch>> {
        let res = self.send_request(BrowserCommand::HandleEvent { 
            element_id, 
            event_name: event_name.to_string(), 
            payload 
        }).await?;
        
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        
        let patches: Vec<hyperlight_protocol::VDomPatch> = serde_json::from_value(
            res.result.unwrap_or(serde_json::json!({"patches": []}))["patches"].clone()
        )?;
        
        Ok(patches)
    }
}
