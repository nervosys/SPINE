use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::ClientConfig;
use rustls::RootCertStore;
use spine_parser::UnifiedRepresentation;
use spine_protocol::connection::Connection;
use spine_protocol::frame::{Frame, FrameType};
use spine_protocol::native;
use spine_protocol::{BrowserCommand, SpineBinary};
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
// The identity key type of `over_with_identity`, and the authority mode a
// server reports, so a caller needs no direct dependency for either.
pub use spine_protocol::hello::AuthorityMode;
pub use spine_protocol::identify::SigningKey;

/// A client of a SPINE daemon, speaking spine-wire v1.
///
/// Requests are serialized on one connection: each call sends one `REQUEST`
/// frame and waits for the `RESPONSE` with the same frame id; no body carries
/// an id. Every constructor performs the `HELLO` handshake and fails with the
/// server's `CLOSE` code and message if the server refuses.
///
/// # Errors
///
/// Methods return `anyhow::Result`. When the daemon answers a command with
/// `{err: {code, message}}`, the error is a
/// [`spine_protocol::native::NativeError::Remote`]; recover the stable `code`
/// with [`spine_protocol::native::remote_code`] (or both fields with
/// [`spine_protocol::native::remote_error`]) and branch on it, never on the
/// text. Connection failures are other `NativeError` variants.
pub struct AgentClient<S> {
    /// The v1 connection, for direct frame access. Use it with care: a
    /// request left unanswered keeps its id in flight until its reply is read.
    pub conn: Connection<S>,
    ping_counter: u64,
}

impl AgentClient<TcpStream> {
    pub async fn connect(addr: &str) -> anyhow::Result<Self> {
        let stream =
            tokio::time::timeout(std::time::Duration::from_secs(10), TcpStream::connect(addr))
                .await
                .map_err(|_| anyhow::anyhow!("native connection deadline exceeded"))??;
        Self::over(stream).await
    }

    /// Like [`AgentClient::connect`], and the client then proves that it holds
    /// `identity` with an `IDENTIFY` frame (see
    /// [`AgentClient::over_with_identity`]).
    pub async fn connect_with_identity(addr: &str, identity: &SigningKey) -> anyhow::Result<Self> {
        let stream =
            tokio::time::timeout(std::time::Duration::from_secs(10), TcpStream::connect(addr))
                .await
                .map_err(|_| anyhow::anyhow!("native connection deadline exceeded"))??;
        Self::over_with_identity(stream, identity).await
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
            let certs: Vec<_> = CertificateDer::pem_reader_iter(std::fs::File::open(ca)?)
                .filter_map(Result::ok)
                .collect();
            for cert in certs {
                root_store.add(cert)?;
            }
        } else {
            // Fallback to system certs if no CA provided
            let loaded = rustls_native_certs::load_native_certs();
            for error in &loaded.errors {
                tracing::warn!(%error, "skipping a system root certificate source");
            }
            if loaded.certs.is_empty() && !loaded.errors.is_empty() {
                anyhow::bail!(
                    "could not load any system root certificates ({} error(s), first: {})",
                    loaded.errors.len(),
                    loaded.errors[0]
                );
            }
            for cert in loaded.certs {
                root_store.add(cert)?;
            }
        }

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = if let Some((cert_path, key_path)) = client_cert {
            let certs: Vec<CertificateDer<'static>> =
                CertificateDer::pem_reader_iter(std::fs::File::open(cert_path)?)
                    .filter_map(Result::ok)
                    .collect();

            let key: PrivateKeyDer<'static> =
                PrivatePkcs8KeyDer::pem_reader_iter(std::fs::File::open(key_path)?)
                    .find_map(Result::ok)
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
            ping_counter: 0,
        })
    }

    /// Like [`AgentClient::over`], and the client then sends `IDENTIFY`,
    /// proving that it holds `identity`'s key (`docs/spec/00-wire.md` 6.6).
    /// The default, [`AgentClient::over`], is anonymous.
    ///
    /// The identity is recorded by the server and grants nothing: the daemon
    /// answers an identified client exactly as it answers an anonymous one.
    /// The proof covers this connection's handshake but not the byte stream,
    /// so a relay between the client and the server is not detected. A server
    /// that cannot verify the proof closes the connection with
    /// `protocol_violation`, which the first later call reports.
    pub async fn over_with_identity(stream: S, identity: &SigningKey) -> anyhow::Result<Self> {
        let conn = native::client_connect_with_identity(stream, Some(identity)).await?;
        Ok(Self {
            conn,
            ping_counter: 0,
        })
    }

    /// The authority mode the server reported in `HELLO_ACK`: `Off` when the
    /// node asks for no grant. Nothing in this client acts on it yet.
    pub fn authority(&self) -> AuthorityMode {
        self.conn.authority()
    }

    /// This connection's `binding`, the 32 bytes the server chose and sent in
    /// `HELLO_ACK`. It grants nothing.
    pub fn binding(&self) -> [u8; 32] {
        self.conn.binding()
    }

    /// The server's clock (Unix seconds) at `HELLO_ACK`; not authenticated.
    pub fn server_time(&self) -> u64 {
        self.conn.server_time()
    }

    /// The public key this client proved with `IDENTIFY`, or `None` for an
    /// anonymous client.
    pub fn identity(&self) -> Option<[u8; 32]> {
        self.conn.identity()
    }

    pub async fn capabilities(
        &mut self,
    ) -> anyhow::Result<spine_protocol::CapabilityAdvertisement> {
        let query = BrowserCommand::CapabilityQuery(spine_protocol::CapabilityQuery {
            selector: spine_protocol::CapabilitySelector::All,
        });
        let advertisement = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            native::call::<_, spine_protocol::CapabilityAdvertisement>(&mut self.conn, &query),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "capability discovery deadline exceeded; reconnect before another request"
            )
        })??;
        Ok(advertisement)
    }

    pub async fn namespace_status(&mut self) -> anyhow::Result<serde_json::Value> {
        self.send_request(BrowserCommand::NamespaceStatus).await
    }

    /// Pin the worker signing key. Never automatically retry a mutation after
    /// uncertain delivery; resubmit the exact same task id and specification.
    ///
    /// A refusal by the daemon, and an error the peer's application returned,
    /// both surface as [`native::NativeError::Remote`]: recover the code with
    /// [`native::remote_code`].
    pub async fn mesh_call(
        &mut self,
        peer_id: &str,
        peer_key: [u8; 32],
        payload: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let value: serde_json::Value = self
            .send_request(BrowserCommand::MeshCall {
                peer_id: peer_id.into(),
                peer_key,
                payload,
            })
            .await?;
        if let Some(error) = value.get("error") {
            // The peer answered, and its application returned `{error: {code,
            // message}}` rather than a result.
            let field = |name: &str| error.get(name).and_then(serde_json::Value::as_str);
            return Err(native::NativeError::Remote {
                code: field("code").unwrap_or("application_error").to_string(),
                message: field("message")
                    .map(str::to_string)
                    .unwrap_or_else(|| error.to_string()),
                details: None,
            }
            .into());
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("application returned no result"))
    }

    /// Send `command` and decode the `ok` value of its reply as a `T`. A reply
    /// of `{err: ...}` is a [`native::NativeError::Remote`] holding the code and
    /// message; the frame id correlates the exchange.
    async fn send_request<T>(&mut self, command: BrowserCommand) -> anyhow::Result<T>
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        Ok(native::call(&mut self.conn, &command).await?)
    }

    /// Invoke a native tool once. Calls are serialized on this client and are
    /// never automatically replayed. Drop the connection after a timeout.
    ///
    /// A tool that fails (an unknown tool, bad arguments, the rate limit) is
    /// `Ok(ToolOutcome::Err { code, .. })`; a refusal by the connection layer or
    /// the decoder (`overloaded`, `unknown_command`, `invalid_request`) is an
    /// error carrying that code, see [`native::remote_code`].
    pub async fn call_tool(
        &mut self,
        name: &str,
        args: serde_json::Value,
    ) -> anyhow::Result<spine_protocol::ToolOutcome> {
        let call = BrowserCommand::ToolCall(spine_protocol::ToolCall {
            name: name.into(),
            args,
            trace: None,
        });
        let reply = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            native::call::<_, serde_json::Value>(&mut self.conn, &call),
        )
        .await
        .map_err(|_| anyhow::anyhow!("tool deadline exceeded; reconnect before another request"))?;
        match reply {
            Ok(content) => Ok(spine_protocol::ToolOutcome::Ok { content }),
            Err(native::NativeError::Remote {
                code,
                message,
                details,
            }) if !spine_protocol::body::PROTOCOL_ERROR_CODES.contains(&code.as_str()) => {
                Ok(spine_protocol::ToolOutcome::Err {
                    code,
                    message,
                    details,
                })
            }
            Err(other) => Err(other.into()),
        }
    }

    #[instrument(skip(self))]
    pub async fn navigate(&mut self, url: &str) -> anyhow::Result<()> {
        let _: serde_json::Value = self
            .send_request(BrowserCommand::Navigate {
                url: url.to_string(),
            })
            .await?;
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn get_ur(&mut self) -> anyhow::Result<UnifiedRepresentation> {
        let value: serde_json::Value = self.send_request(BrowserCommand::GetUR).await?;
        let ur: UnifiedRepresentation = serde_json::from_value(value)?;
        Ok(ur)
    }

    pub async fn get_raw_html(&mut self) -> anyhow::Result<String> {
        let value: serde_json::Value = self.send_request(BrowserCommand::GetRawHTML).await?;
        let html = value["html"].as_str().unwrap_or_default().to_string();
        Ok(html)
    }

    pub async fn execute_binary(
        &mut self,
        binary: SpineBinary,
    ) -> anyhow::Result<serde_json::Value> {
        self.send_request(BrowserCommand::ExecuteBinary(binary))
            .await
    }

    #[instrument(skip(self))]
    pub async fn execute_hls(
        &mut self,
        script: &str,
    ) -> anyhow::Result<spine_wasm::WasmExecutionResult> {
        let binary = spine_compiler::Compiler::compile(script)?;
        let value: serde_json::Value = self
            .send_request(BrowserCommand::ExecuteBinary(binary))
            .await?;
        let result: spine_wasm::WasmExecutionResult = serde_json::from_value(value)?;
        Ok(result)
    }

    pub async fn store_knowledge(
        &mut self,
        key: &str,
        value: serde_json::Value,
        tags: Vec<String>,
    ) -> anyhow::Result<()> {
        let _: serde_json::Value = self
            .send_request(BrowserCommand::StoreKnowledge {
                key: key.to_string(),
                value,
                tags,
            })
            .await?;
        Ok(())
    }

    pub async fn query_knowledge(
        &mut self,
        query: &str,
        tags: Vec<String>,
        limit: usize,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let value: serde_json::Value = self
            .send_request(BrowserCommand::QueryKnowledge {
                query: query.to_string(),
                tags,
                limit,
            })
            .await?;
        let results = value["results"].as_array().cloned().unwrap_or_default();
        Ok(results)
    }

    #[instrument(skip(self))]
    pub async fn ping(&mut self) -> anyhow::Result<u64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;

        // A PING frame whose token is the send time in milliseconds; the
        // server's connection layer echoes it in a PONG. The id is a counter
        // of this client's pings, never zero.
        self.ping_counter += 1;
        let id = self.ping_counter;
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

    /// The capabilities the session's current binary declared, as the
    /// program listed them. This is what the program asked for, not a
    /// grant: the daemon checks each action the program requests against
    /// the list (`network` for a navigation, `search`, `storage`), and an
    /// action without its capability is refused with `capability_denied`.
    pub async fn get_binary_capabilities(&mut self) -> anyhow::Result<Vec<String>> {
        let value: serde_json::Value = self
            .send_request(BrowserCommand::GetBinaryCapabilities)
            .await?;
        let capabilities: Vec<String> = serde_json::from_value(value["capabilities"].clone())?;
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
        let value: serde_json::Value = self
            .send_request(BrowserCommand::ResolveName {
                name: name.to_string(),
                if_none_match,
            })
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    /// Resolve many names in one round trip.
    ///
    /// Each name resolves independently, so one bad name does not fail the
    /// batch — the returned vector is positional and carries per-name outcomes.
    pub async fn resolve_names(
        &mut self,
        names: &[String],
    ) -> anyhow::Result<Vec<spine_protocol::NameResolution>> {
        let body: serde_json::Value = self
            .send_request(BrowserCommand::ResolveNames {
                names: names.to_vec(),
            })
            .await?;
        Ok(serde_json::from_value(body["resolutions"].clone())?)
    }

    /// Find providers of a capability, without consulting a central index.
    pub async fn find_providers(
        &mut self,
        capability: &str,
        limit: Option<usize>,
    ) -> anyhow::Result<serde_json::Value> {
        self.send_request(BrowserCommand::FindProviders {
            capability: capability.to_string(),
            limit,
        })
        .await
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
        self.send_request(BrowserCommand::PublishName { record })
            .await
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
        self.send_request(BrowserCommand::FetchName {
            name: name.to_string(),
            if_none_match,
        })
        .await
    }

    /// Walk the agent web from a seed name, bounded by depth and visit count.
    pub async fn crawl_names(
        &mut self,
        seed: &str,
        max_depth: Option<u32>,
        max_visits: Option<usize>,
    ) -> anyhow::Result<serde_json::Value> {
        self.send_request(BrowserCommand::CrawlNames {
            seed: seed.to_string(),
            max_depth,
            max_visits,
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spine_protocol::WireError;

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
        // The wire form of a command: the text of a unit command, a
        // single-entry map for one with fields.
        let cmd = BrowserCommand::Navigate {
            url: "https://example.com".to_string(),
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"navigate": {"url": "https://example.com"}})
        );

        let cmd2 = BrowserCommand::GetUR;
        let json2 = serde_json::to_value(&cmd2).unwrap();
        assert_eq!(json2, serde_json::json!("get_ur"));
    }

    #[test]
    fn test_command_and_reply_codecs() {
        let command = BrowserCommand::GetBinaryCapabilities;
        let body = native::encode_command(&command).unwrap();
        assert_eq!(native::decode_command(&body).unwrap(), command);

        let reply = spine_protocol::Reply::Ok(serde_json::json!({"capabilities": ["navigate"]}));
        assert!(reply.into_result().is_ok());
    }

    #[test]
    fn test_ping_message() {
        let msg = spine_protocol::Message::Ping {
            timestamp: 1234567890,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let rt: spine_protocol::Message = serde_json::from_str(&json).unwrap();
        match rt {
            spine_protocol::Message::Ping { timestamp } => assert_eq!(timestamp, 1234567890),
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

    type Answer = Result<serde_json::Value, WireError>;

    /// A v1 server that answers every `REQUEST` with `reply(command)` until the
    /// client hangs up. A capability query is answered with an advertisement.
    fn spawn_server(
        io: DuplexStream,
        reply: fn(BrowserCommand) -> Answer,
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
                let command = native::decode_command(&frame.body).unwrap();
                match command {
                    BrowserCommand::CapabilityQuery(_) => {
                        let ad = spine_protocol::CapabilityAdvertisement {
                            agent_id: "test".to_string(),
                            capabilities: Vec::new(),
                        };
                        native::send_ok(&mut conn, frame.request_id, &ad)
                            .await
                            .unwrap();
                    }
                    other => match reply(other) {
                        Ok(value) => native::send_ok(&mut conn, frame.request_id, &value)
                            .await
                            .unwrap(),
                        Err(error) => native::send_err(&mut conn, frame.request_id, &error)
                            .await
                            .unwrap(),
                    },
                }
            }
        })
    }

    fn answer(command: BrowserCommand) -> Answer {
        match command {
            BrowserCommand::ToolCall(call) => Ok(call.args),
            _ => Ok(serde_json::json!({"html": "<p>hi</p>"})),
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
        assert_eq!(
            result,
            spine_protocol::ToolOutcome::Ok {
                content: serde_json::json!({"text": "x"})
            }
        );

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
    async fn a_client_connects_anonymous_by_default_and_with_an_identity_on_request() {
        for identified in [false, true] {
            let (client_io, server_io) = duplex(64 * 1024);
            let server = spawn_server(server_io, answer);
            let key = SigningKey::from_bytes(&[0x55; 32]);
            let mut client = tokio::time::timeout(LIMIT, async {
                if identified {
                    AgentClient::over_with_identity(client_io, &key).await
                } else {
                    AgentClient::over(client_io).await
                }
            })
            .await
            .unwrap()
            .unwrap();
            assert_eq!(
                client.identity(),
                identified.then(|| key.verifying_key().to_bytes())
            );
            assert_eq!(client.authority(), AuthorityMode::Off);
            assert_ne!(client.binding(), [0u8; 32]);
            assert!(client.server_time() > 1_600_000_000);
            let html = tokio::time::timeout(LIMIT, client.get_raw_html())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(html, "<p>hi</p>");
            drop(client);
            tokio::time::timeout(LIMIT, server).await.unwrap().unwrap();
        }
    }

    #[tokio::test]
    async fn an_error_response_surfaces_with_its_code() {
        fn refuse(_: BrowserCommand) -> Answer {
            Err(WireError::new("rate_limit", "Rate limit exceeded"))
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
        assert_eq!(native::remote_code(&error), Some("rate_limit"));
        assert_eq!(
            native::remote_error(&error),
            Some(("rate_limit", "Rate limit exceeded"))
        );
        // A tool that fails is an outcome, not an error, unless the refusal is
        // the decoder's or the connection layer's.
        let outcome = tokio::time::timeout(LIMIT, client.call_tool("shell", serde_json::json!({})))
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(&outcome, spine_protocol::ToolOutcome::Err { code, .. } if code == "rate_limit")
        );
        drop(client);
        tokio::time::timeout(LIMIT, server).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn protocol_refusals_of_a_tool_call_are_errors() {
        fn invalid(_: BrowserCommand) -> Answer {
            Err(WireError::new("invalid_request", "bad field"))
        }
        let (client_io, server_io) = duplex(64 * 1024);
        let server = spawn_server(server_io, invalid);
        let mut client = tokio::time::timeout(LIMIT, AgentClient::over(client_io))
            .await
            .unwrap()
            .unwrap();
        let error = tokio::time::timeout(LIMIT, client.call_tool("t", serde_json::json!({})))
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(native::remote_code(&error), Some("invalid_request"));
        drop(client);
        tokio::time::timeout(LIMIT, server).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn an_application_error_inside_a_mesh_call_keeps_its_code() {
        fn denied(_: BrowserCommand) -> Answer {
            Ok(serde_json::json!({"error": {"code": "not_authorized", "message": "no"}}))
        }
        let (client_io, server_io) = duplex(64 * 1024);
        let server = spawn_server(server_io, denied);
        let mut client = tokio::time::timeout(LIMIT, AgentClient::over(client_io))
            .await
            .unwrap()
            .unwrap();
        let error = tokio::time::timeout(
            LIMIT,
            client.mesh_call("peer", [0; 32], serde_json::json!({})),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(native::remote_code(&error), Some("not_authorized"));
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
