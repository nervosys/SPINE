use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::ClientConfig;
use rustls::RootCertStore;
use spine_parser::UnifiedRepresentation;
use spine_protocol::connection::Connection;
use spine_protocol::frame::{Frame, FrameType};
use spine_protocol::native;
use spine_protocol::{BrowserCommand, Message, Request, Response, SpineBinary};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tracing::instrument;
pub mod resources;

// Re-export the compiler for convenience
pub use spine_compiler::Compiler;
// Re-export WebSocket stream for CLI and external consumers
pub use spine_transport::WebSocketClientStream;

/// A client of a SPINE daemon, speaking spine-wire v1.
///
/// Requests are serialized on one connection: each call sends one `REQUEST`
/// frame and waits for the `RESPONSE` with the same frame id. The `id` fields
/// inside the message bodies are carried but not cross-checked; the frame id
/// is what correlates. Every constructor performs the `HELLO` handshake and
/// fails with the server's `CLOSE` code and message if the server refuses.
pub struct AgentClient<S> {
    /// The v1 connection, for direct frame access. Use it with care: a
    /// request left unanswered keeps its id in flight until its reply is read.
    pub conn: Connection<S>,
    request_counter: u64,
}

impl AgentClient<TcpStream> {
    pub async fn connect(addr: &str) -> anyhow::Result<Self> {
        let stream =
            tokio::time::timeout(std::time::Duration::from_secs(10), TcpStream::connect(addr))
                .await
                .map_err(|_| anyhow::anyhow!("native connection deadline exceeded"))??;
        Self::over(stream).await
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

        let provider = Arc::new(rustls::crypto::ring::default_provider());
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

            ClientConfig::builder_with_provider(provider.clone())
                .with_safe_default_protocol_versions()?
                .with_root_certificates(root_store)
                .with_client_auth_cert(certs, key)?
        } else {
            ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()?
                .with_root_certificates(root_store)
                .with_no_client_auth()
        };

        let connector = TlsConnector::from(Arc::new(config));
        let stream =
            tokio::time::timeout(std::time::Duration::from_secs(10), TcpStream::connect(addr))
                .await
                .map_err(|_| anyhow::anyhow!("native connection deadline exceeded"))??;
        let domain = ServerName::try_from(domain.to_owned())?;
        let tls_stream = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            connector.connect(domain, stream),
        )
        .await
        .map_err(|_| anyhow::anyhow!("native TLS handshake deadline exceeded"))??;

        Self::over(tls_stream).await
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
        Self::over(stream).await
    }
}

impl<S> AgentClient<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    /// Run the spine-wire v1 handshake over an already-connected `stream`.
    ///
    /// Fails if the server answers with `CLOSE` (the error names its code and
    /// message), sends something other than `HELLO_ACK`, or does not answer
    /// within the handshake deadline of ten seconds.
    pub async fn over(stream: S) -> anyhow::Result<Self> {
        let conn = native::client_connect(stream).await?;
        Ok(Self {
            conn,
            request_counter: 0,
        })
    }

    pub async fn capabilities(
        &mut self,
    ) -> anyhow::Result<spine_protocol::CapabilityAdvertisement> {
        self.request_counter += 1;
        let id = format!("cap-{}", self.request_counter);
        let query = Message::CapabilityQuery(spine_protocol::CapabilityQuery {
            id,
            selector: spine_protocol::CapabilitySelector::All,
        });
        let reply = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            native::call(&mut self.conn, &query),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "capability discovery deadline exceeded; reconnect before another request"
            )
        })??;
        match reply {
            Message::CapabilityAd(advertisement) => Ok(advertisement),
            Message::Response(Response {
                error: Some(error), ..
            }) => anyhow::bail!(error),
            _ => anyhow::bail!("unexpected reply to a capability query"),
        }
    }

    pub async fn namespace_status(&mut self) -> anyhow::Result<serde_json::Value> {
        let response = self.send_request(BrowserCommand::NamespaceStatus).await?;
        if let Some(error) = response.error {
            anyhow::bail!(error);
        }
        response
            .result
            .ok_or_else(|| anyhow::anyhow!("namespace status returned no result"))
    }

    /// Pin the worker signing key. Never automatically retry a mutation after
    /// uncertain delivery; resubmit the exact same task id and specification.
    pub async fn mesh_call(
        &mut self,
        peer_id: &str,
        peer_key: [u8; 32],
        payload: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let response = self
            .send_request(BrowserCommand::MeshCall {
                peer_id: peer_id.into(),
                peer_key,
                payload,
            })
            .await?;
        if let Some(error) = response.error {
            anyhow::bail!("mesh call: {error}");
        }
        let value = response
            .result
            .ok_or_else(|| anyhow::anyhow!("mesh call returned no result"))?;
        if let Some(error) = value.get("error") {
            anyhow::bail!("application: {error}");
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("application returned no result"))
    }

    async fn send_request(&mut self, command: BrowserCommand) -> anyhow::Result<Response> {
        self.request_counter += 1;
        let id = self.request_counter.to_string();
        let req = Message::Request(Request { id, command });
        match native::call(&mut self.conn, &req).await? {
            Message::Response(res) => Ok(res),
            _ => anyhow::bail!("unexpected reply to a request"),
        }
    }

    /// Invoke a native tool once. Calls are serialized on this client and are
    /// never automatically replayed. Drop the connection after a timeout.
    pub async fn call_tool(
        &mut self,
        name: &str,
        args: serde_json::Value,
    ) -> anyhow::Result<spine_protocol::ToolResult> {
        self.request_counter += 1;
        let id = format!("tool-{}", self.request_counter);
        let call = Message::ToolCall(spine_protocol::ToolCall {
            id,
            name: name.into(),
            args,
            trace: None,
        });
        let reply = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            native::call(&mut self.conn, &call),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!("tool deadline exceeded; reconnect before another request")
        })??;
        match reply {
            Message::ToolResult(result) => Ok(result),
            Message::Response(Response {
                error: Some(error), ..
            }) => anyhow::bail!(error),
            _ => anyhow::bail!("unexpected reply to a tool call"),
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

    #[instrument(skip(self))]
    pub async fn ping(&mut self) -> anyhow::Result<u64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;

        // A PING frame whose token is the send time in milliseconds; the
        // server's connection layer echoes it in a PONG.
        self.request_counter += 1;
        let id = self.request_counter;
        let ping = Frame {
            frame_type: FrameType::Ping,
            request_id: id,
            body: native::ping_body(now)?,
        };
        native::send_bounded(&mut self.conn, ping).await?;

        loop {
            match self.conn.recv().await? {
                Some(frame) if frame.frame_type == FrameType::Pong && frame.request_id == id => {
                    let token = native::ping_token(&frame.body)?;
                    let end = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_millis() as u64;
                    return Ok(end.saturating_sub(token));
                }
                Some(_) => {}
                None => anyhow::bail!("connection closed before the pong"),
            }
        }
    }

    pub async fn get_binary_capabilities(&mut self) -> anyhow::Result<Vec<String>> {
        let res = self
            .send_request(BrowserCommand::GetBinaryCapabilities)
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let capabilities: Vec<String> =
            serde_json::from_value(res.result.unwrap_or_default()["capabilities"].clone())?;
        Ok(capabilities)
    }

    // ---- Namespace ----
    //
    // `navigate` fetches a URL from the human web. Everything below operates on
    // `spine://` names, which resolve through SPINE's own namespace and verify
    // against their own authority — no CA, and no host to trust.

    /// Resolve a `spine://` name to its signed record.
    ///
    /// `if_none_match` carries a content hash the caller already holds; when it
    /// still matches, the answer is [`NameResolution::Unchanged`] with no body.
    /// It is an ETag whose value the caller can verify, rather than an opaque
    /// token chosen by whoever is answering.
    pub async fn resolve_name(
        &mut self,
        name: &str,
        if_none_match: Option<[u8; 32]>,
    ) -> anyhow::Result<spine_protocol::NameResolution> {
        let res = self
            .send_request(BrowserCommand::ResolveName {
                name: name.to_string(),
                if_none_match,
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(serde_json::from_value(res.result.unwrap_or_default())?)
    }

    /// Resolve many names in one round trip.
    ///
    /// Each name resolves independently, so one bad name does not fail the
    /// batch — the returned vector is positional and carries per-name outcomes.
    pub async fn resolve_names(
        &mut self,
        names: &[String],
    ) -> anyhow::Result<Vec<spine_protocol::NameResolution>> {
        let res = self
            .send_request(BrowserCommand::ResolveNames {
                names: names.to_vec(),
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        let body = res.result.unwrap_or_default();
        Ok(serde_json::from_value(body["resolutions"].clone())?)
    }

    /// Find providers of a capability, without consulting a central index.
    pub async fn find_providers(
        &mut self,
        capability: &str,
        limit: Option<usize>,
    ) -> anyhow::Result<serde_json::Value> {
        let res = self
            .send_request(BrowserCommand::FindProviders {
                capability: capability.to_string(),
                limit,
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(res.result.unwrap_or_default())
    }

    /// Publish a signed record.
    ///
    /// The record carries its own signature, so the receiver needs no prior
    /// trust relationship with the publisher — and will reject it outright if
    /// the signature does not verify against the name.
    pub async fn publish_name(
        &mut self,
        record: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let res = self
            .send_request(BrowserCommand::PublishName { record })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(res.result.unwrap_or_default())
    }

    /// Resolve a name and get back the endpoints to dial, in priority order.
    ///
    /// Deliberately stops short of fetching the bytes: conflating resolution
    /// and transfer would hide which of the two failed.
    pub async fn fetch_name(
        &mut self,
        name: &str,
        if_none_match: Option<[u8; 32]>,
    ) -> anyhow::Result<serde_json::Value> {
        let res = self
            .send_request(BrowserCommand::FetchName {
                name: name.to_string(),
                if_none_match,
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(res.result.unwrap_or_default())
    }

    /// Walk the agent web from a seed name, bounded by depth and visit count.
    pub async fn crawl_names(
        &mut self,
        seed: &str,
        max_depth: Option<u32>,
        max_visits: Option<usize>,
    ) -> anyhow::Result<serde_json::Value> {
        let res = self
            .send_request(BrowserCommand::CrawlNames {
                seed: seed.to_string(),
                max_depth,
                max_visits,
            })
            .await?;
        if let Some(err) = res.error {
            anyhow::bail!(err);
        }
        Ok(res.result.unwrap_or_default())
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
            command: BrowserCommand::GetBinaryCapabilities,
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
            command: BrowserCommand::GetUR,
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

    // ---- spine-wire v1 over an in-process stream ----

    use std::time::Duration;
    use tokio::io::{duplex, DuplexStream};

    const LIMIT: Duration = Duration::from_secs(20);

    /// A v1 server that answers every `REQUEST` with `reply(request)` until the
    /// client hangs up.
    fn spawn_server(
        io: DuplexStream,
        reply: fn(Message) -> Message,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut conn = Connection::server_handshake(
                io,
                &native::local_config(),
                native::DEFAULT_HELLO_DEADLINE,
            )
            .await
            .unwrap();
            while let Ok(Some(frame)) = conn.recv().await {
                if frame.frame_type != FrameType::Request {
                    continue;
                }
                let request = native::decode_request(&frame.body).unwrap();
                native::send_reply(&mut conn, frame.request_id, &reply(request))
                    .await
                    .unwrap();
            }
        })
    }

    fn answer(request: Message) -> Message {
        match request {
            Message::Request(r) => Message::Response(Response {
                id: r.id,
                result: Some(serde_json::json!({"html": "<p>hi</p>"})),
                error: None,
            }),
            Message::ToolCall(call) => Message::ToolResult(spine_protocol::ToolResult {
                id: call.id,
                trace: None,
                outcome: spine_protocol::ToolOutcome::Ok { content: call.args },
            }),
            Message::CapabilityQuery(query) => {
                Message::CapabilityAd(spine_protocol::CapabilityAdvertisement {
                    id: query.id,
                    agent_id: "test".to_string(),
                    capabilities: Vec::new(),
                })
            }
            other => panic!("unexpected request {other:?}"),
        }
    }

    #[tokio::test]
    async fn client_round_trips_requests_tools_capabilities_and_pings() {
        let (client_io, server_io) = duplex(64 * 1024);
        let server = spawn_server(server_io, answer);
        let mut client = tokio::time::timeout(LIMIT, AgentClient::over(client_io))
            .await
            .unwrap()
            .unwrap();

        let html = tokio::time::timeout(LIMIT, client.get_raw_html())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(html, "<p>hi</p>");

        let result = tokio::time::timeout(
            LIMIT,
            client.call_tool("spine.content/hash", serde_json::json!({"text": "x"})),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(
            result.outcome,
            spine_protocol::ToolOutcome::Ok { .. }
        ));

        let ads = tokio::time::timeout(LIMIT, client.capabilities())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ads.agent_id, "test");

        tokio::time::timeout(LIMIT, client.ping())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(client.conn.in_flight_len(), 0);

        drop(client);
        tokio::time::timeout(LIMIT, server).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn an_error_response_surfaces_as_an_error() {
        fn refuse(request: Message) -> Message {
            match request {
                Message::Request(r) => Message::Response(Response {
                    id: r.id,
                    result: None,
                    error: Some("Rate limit exceeded".to_string()),
                }),
                other => panic!("unexpected request {other:?}"),
            }
        }
        let (client_io, server_io) = duplex(64 * 1024);
        let server = spawn_server(server_io, refuse);
        let mut client = tokio::time::timeout(LIMIT, AgentClient::over(client_io))
            .await
            .unwrap()
            .unwrap();
        let error = tokio::time::timeout(LIMIT, client.get_raw_html())
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("Rate limit exceeded"));
        drop(client);
        tokio::time::timeout(LIMIT, server).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn connect_reports_the_servers_close_code() {
        let (client_io, mut server_io) = duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let hello = spine_protocol::frame::read_frame(
                &mut server_io,
                spine_protocol::frame::PRE_HELLO_MAX_FRAME,
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(hello.frame_type, FrameType::Hello);
            let close = spine_protocol::hello::Close::new(
                spine_protocol::frame::CloseCode::ShuttingDown,
                "draining",
            );
            spine_protocol::frame::write_frame(
                &mut server_io,
                &close.to_frame().unwrap(),
                spine_protocol::frame::PRE_HELLO_MAX_FRAME,
                false,
            )
            .await
            .unwrap();
        });
        let error = tokio::time::timeout(LIMIT, AgentClient::over(client_io))
            .await
            .unwrap()
            .err()
            .expect("the handshake must fail");
        let text = error.to_string();
        assert!(
            text.contains("shutting_down") && text.contains("draining"),
            "{text}"
        );
        tokio::time::timeout(LIMIT, server).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn connect_to_a_peer_that_is_not_spine_fails() {
        // A pre-v1 style peer: it answers a HELLO with something that is not
        // a HELLO_ACK, so the handshake ends in an error and not a hang.
        let (client_io, mut server_io) = duplex(64 * 1024);
        let server = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            server_io.write_all(&[0xff; 32]).await.unwrap();
            // Keep the stream open until the client has given up.
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let result = tokio::time::timeout(LIMIT, AgentClient::over(client_io))
            .await
            .unwrap();
        assert!(result.is_err());
        server.abort();
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
