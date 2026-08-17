//! WebSocket transport for the mesh.
//!
//! The same framing, encryption, reply plumbing, and pooling as
//! [`crate::mesh_tcp`] — [`attach`] and [`attach_secure`] are generic over the
//! stream, so only connection establishment differs. What WebSocket buys is
//! reach: it traverses the corporate proxies and firewalls that drop a bare TCP
//! connection to port 9440, and it is the only transport a browser-resident
//! agent can open at all. For an agent web whose weakest axis is interop, being
//! dialable from where agents actually run matters more than shaving a frame
//! header.
//!
//! WebSocket already frames messages, so the length prefix underneath is
//! redundant. It is kept anyway: sharing one framing path with TCP and QUIC
//! means a frame that parses on one transport parses identically on all of
//! them, and a bug found in one is fixed for all three. The cost is four bytes.

use dashmap::DashMap;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use ed25519_dalek::SigningKey;
use spine_transport::websocket::{
    WebSocketBridge, WebSocketClientStream, WebSocketServerBridge, WebSocketStream,
};

use crate::mesh_tcp::{
    attach, attach_secure, client_handshake, server_handshake, Inbound, ReplyPath, Security,
    INBOUND_QUEUE,
};
use crate::naming_mesh::{MeshNameResolver, NameMeshError, NameTransport};
use crate::AgentId;
use crate::mesh::MeshEnvelope;

use async_trait::async_trait;

/// A pooled WebSocket transport for mesh envelopes.
#[derive(Debug)]
pub struct WsNameTransport {
    /// `ws://host:port/path` per peer.
    urls: DashMap<AgentId, String>,
    /// Expected mesh identity per peer, used to pin the handshake.
    identities: DashMap<AgentId, [u8; 32]>,
    /// Live outbound connections.
    pool: DashMap<AgentId, ReplyPath>,
    /// Connections opened to seed URLs during bootstrap, keyed by URL because
    /// the peer behind them has no known identity yet.
    bootstrap: DashMap<String, ReplyPath>,
    inbound: mpsc::Sender<Inbound>,
    security: Security,
}

impl WsNameTransport {
    /// A plaintext WebSocket transport.
    pub fn new() -> (Arc<Self>, mpsc::Receiver<Inbound>) {
        Self::with_security(Security::Plaintext)
    }

    /// A WebSocket transport whose connections are encrypted and mutually
    /// authenticated against the node's mesh identity.
    ///
    /// Worth doing even against a `wss://` endpoint: TLS authenticates the
    /// *server operator*, while this authenticates the *mesh identity* that
    /// signs the records — which is the thing a resolver actually needs to
    /// trust, and it holds end-to-end through any proxy in between.
    pub fn encrypted(signing: SigningKey) -> (Arc<Self>, mpsc::Receiver<Inbound>) {
        Self::with_security(Security::Encrypted(Box::new(
            crate::mesh_tcp::EncryptionConfig { signing },
        )))
    }

    pub fn with_security(security: Security) -> (Arc<Self>, mpsc::Receiver<Inbound>) {
        let (tx, rx) = mpsc::channel(INBOUND_QUEUE);
        let transport = Arc::new(Self {
            urls: DashMap::new(),
            identities: DashMap::new(),
            pool: DashMap::new(),
            bootstrap: DashMap::new(),
            inbound: tx,
            security,
        });
        (transport, rx)
    }

    /// A sender for the same inbound stream, for a listener to share.
    pub fn inbound_sender(&self) -> mpsc::Sender<Inbound> {
        self.inbound.clone()
    }

    /// Record a peer's WebSocket URL.
    pub fn set_url(&self, agent: AgentId, url: impl Into<String>) {
        let url = url.into();
        let previous = self.urls.insert(agent, url.clone());
        // Only a genuine change invalidates the pooled connection; gossip
        // re-announces the same URL constantly.
        if previous.as_deref() != Some(url.as_str()) {
            self.pool.remove(&agent);
        }
    }

    /// Record the mesh identity expected at a peer, so the handshake can pin it.
    pub fn set_identity(&self, agent: AgentId, identity: [u8; 32]) {
        self.identities.insert(agent, identity);
    }

    pub fn known_urls(&self) -> usize {
        self.urls.len()
    }

    pub fn pooled_connections(&self) -> usize {
        self.pool.len()
    }

    pub fn evict(&self, agent: &AgentId) {
        self.pool.remove(agent);
    }

    pub fn is_encrypted(&self) -> bool {
        matches!(self.security, Security::Encrypted(_))
    }

    async fn connection(&self, agent: &AgentId) -> Result<ReplyPath, NameMeshError> {
        if let Some(existing) = self.pool.get(agent) {
            return Ok(existing.value().clone());
        }
        let url = self
            .urls
            .get(agent)
            .map(|e| e.value().clone())
            .ok_or_else(|| NameMeshError::Transport(format!("no URL known for {agent:?}")))?;

        let bridge = WebSocketBridge::connect(&url)
            .await
            .map_err(|e| NameMeshError::Transport(format!("ws connect {url}: {e}")))?;
        let mut stream = WebSocketClientStream::new(bridge);

        let transport = match &self.security {
            Security::Plaintext => ReplyPath::Plain(attach(stream, self.inbound.clone())),
            Security::Encrypted(cfg) => {
                let expected = self.identities.get(agent).map(|e| *e.value());
                let session =
                    client_handshake(&mut stream, &cfg.signing, expected.as_ref()).await?;
                ReplyPath::Secure(attach_secure(stream, session, self.inbound.clone()))
            }
        };
        self.pool.insert(*agent, transport.clone());
        Ok(transport)
    }

    async fn deliver(&self, target: AgentId, envelope: &MeshEnvelope) -> Result<(), NameMeshError> {
        let conn = self.connection(&target).await?;
        if conn.send(envelope.clone()).await.is_ok() {
            return Ok(());
        }
        // Stale pooled socket — most often a peer that restarted. One retry on a
        // fresh connection, then report honestly.
        self.pool.remove(&target);
        let conn = self.connection(&target).await?;
        conn.send(envelope.clone()).await
    }

    /// Connect to a seed URL whose mesh identity is not yet known.
    ///
    /// Keyed by URL rather than pooled by agent id, and unpinned even when
    /// encrypted, for the same reason as the TCP path: the identity is what the
    /// exchange exists to establish. See [`TcpNameTransport::provisional`].
    ///
    /// [`TcpNameTransport::provisional`]: crate::mesh_tcp::TcpNameTransport
    async fn provisional(&self, endpoint: &str) -> Result<ReplyPath, NameMeshError> {
        if let Some(existing) = self.bootstrap.get(endpoint) {
            return Ok(existing.value().clone());
        }

        let bridge = WebSocketBridge::connect(endpoint)
            .await
            .map_err(|e| NameMeshError::Transport(format!("ws connect seed {endpoint}: {e}")))?;
        let mut stream = WebSocketClientStream::new(bridge);

        let path = match &self.security {
            Security::Plaintext => ReplyPath::Plain(attach(stream, self.inbound.clone())),
            Security::Encrypted(cfg) => {
                // Nothing to pin to: establishing the identity is the point
                // of dialing a bootstrap address.
                let session = client_handshake(&mut stream, &cfg.signing, None).await?;
                ReplyPath::Secure(attach_secure(stream, session, self.inbound.clone()))
            }
        };
        self.bootstrap.insert(endpoint.to_string(), path.clone());
        Ok(path)
    }
}

#[async_trait]
impl NameTransport for WsNameTransport {
    async fn send(&self, envelope: MeshEnvelope) -> Result<(), NameMeshError> {
        match envelope.to {
            crate::mesh::MeshTarget::Agent(id) => self.deliver(id, &envelope).await,
            _ => {
                let peers: Vec<AgentId> = self.urls.iter().map(|e| *e.key()).collect();
                for peer in peers {
                    let mut addressed = envelope.clone();
                    addressed.to = crate::mesh::MeshTarget::Agent(peer);
                    let _ = self.deliver(peer, &addressed).await;
                }
                Ok(())
            }
        }
    }

    async fn send_to(&self, endpoint: &str, envelope: MeshEnvelope) -> Result<(), NameMeshError> {
        self.provisional(endpoint).await?.send(envelope).await
    }

    /// Take the first endpoint spelled as a WebSocket URL, ignoring bare socket
    /// addresses meant for another transport.
    async fn learn(&self, agent: AgentId, endpoints: &[String]) {
        if let Some(url) = endpoints
            .iter()
            .find(|e| e.starts_with("ws://") || e.starts_with("wss://"))
        {
            self.set_url(agent, url.clone());
        }
    }

    async fn release(&self, endpoint: &str) {
        self.bootstrap.remove(endpoint);
    }
}

/// Accepts WebSocket upgrades and feeds them to the inbound stream.
#[derive(Debug)]
pub struct WsListener {
    listener: TcpListener,
}

impl WsListener {
    /// Bind. Pass port 0 to let the OS choose.
    pub async fn bind(addr: std::net::SocketAddr) -> Result<Self, NameMeshError> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| NameMeshError::Transport(format!("bind {addr}: {e}")))?;
        Ok(Self { listener })
    }

    pub fn local_addr(&self) -> Result<std::net::SocketAddr, NameMeshError> {
        self.listener
            .local_addr()
            .map_err(|e| NameMeshError::Transport(format!("local_addr: {e}")))
    }

    /// Accept until the task is dropped.
    ///
    /// The upgrade and any handshake run on the connection's own task, so a peer
    /// that stalls mid-negotiation cannot hold up the accept loop.
    pub async fn serve(self, inbound: mpsc::Sender<Inbound>, signing: Option<SigningKey>) {
        loop {
            let (tcp, peer) = match self.listener.accept().await {
                Ok(pair) => pair,
                Err(e) => {
                    tracing::warn!("ws accept failed: {e}");
                    continue;
                }
            };
            let _ = tcp.set_nodelay(true);
            let inbound = inbound.clone();
            let signing = signing.clone();

            tokio::spawn(async move {
                match upgrade(tcp, inbound, signing).await {
                    Ok(()) => {}
                    Err(e) => tracing::debug!("ws connection from {peer} rejected: {e}"),
                }
            });
        }
    }
}

/// Upgrade one accepted TCP connection and attach it.
async fn upgrade(
    tcp: TcpStream,
    inbound: mpsc::Sender<Inbound>,
    signing: Option<SigningKey>,
) -> Result<(), NameMeshError> {
    let bridge = WebSocketServerBridge::accept(tcp)
        .await
        .map_err(|e| NameMeshError::Transport(format!("ws upgrade: {e}")))?;
    let mut stream = WebSocketStream::new(bridge);

    match signing {
        None => {
            attach(stream, inbound);
        }
        Some(key) => {
            let session = server_handshake(&mut stream, &key).await?;
            attach_secure(stream, session, inbound);
        }
    }
    Ok(())
}

/// Bind a WebSocket listener, wire it to `resolver`, and start serving.
pub async fn serve_ws_node(
    bind_addr: std::net::SocketAddr,
    resolver: Arc<MeshNameResolver>,
    transport: Arc<WsNameTransport>,
    inbound: mpsc::Receiver<Inbound>,
    signing: Option<SigningKey>,
) -> Result<std::net::SocketAddr, NameMeshError> {
    let listener = WsListener::bind(bind_addr).await?;
    let addr = listener.local_addr()?;
    tokio::spawn(listener.serve(transport.inbound_sender(), signing));
    tokio::spawn(crate::mesh_tcp::pump(inbound, resolver));
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::SigningIdentity;
    use crate::mesh::{MeshConfig, MeshNode};
    use spine_name::{Endpoint, NameRecord, SpineUri};
    use std::time::Duration;

    const NOW: u64 = 1_700_000_000;

    fn record(seed: u8, caps: &[&str]) -> NameRecord {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name, 1, NOW)
            .unwrap()
            .with_ttl(3600)
            .with_endpoint(Endpoint::new("ws", format!("10.0.0.{seed}:9440")));
        for c in caps {
            rec = rec.with_capability(*c);
        }
        rec.sign(&key).unwrap();
        rec
    }

    struct WsNode {
        resolver: Arc<MeshNameResolver>,
        transport: Arc<WsNameTransport>,
        mesh: Arc<MeshNode>,
        url: String,
        signing: SigningKey,
    }

    async fn spawn_ws(seed: u8, encrypted: bool) -> WsNode {
        let identity = SigningIdentity::from_seed(&format!("ws{seed}"), [150 + seed; 32]);
        let signing = SigningKey::from_bytes(&[150 + seed; 32]);
        let mesh = Arc::new(MeshNode::new(identity, MeshConfig::default()));

        let (transport, inbound) = if encrypted {
            WsNameTransport::encrypted(signing.clone())
        } else {
            WsNameTransport::new()
        };
        let resolver = Arc::new(
            MeshNameResolver::new(mesh.clone(), transport.clone())
                .with_clock(|| NOW)
                .with_timeout(Duration::from_secs(5)),
        );
        let addr = serve_ws_node(
            "127.0.0.1:0".parse().unwrap(),
            resolver.clone(),
            transport.clone(),
            inbound,
            encrypted.then(|| signing.clone()),
        )
        .await
        .unwrap();

        WsNode {
            resolver,
            transport,
            mesh,
            url: format!("ws://{addr}/"),
            signing,
        }
    }

    async fn introduce(a: &WsNode, b: &WsNode) {
        a.transport.set_url(*b.mesh.agent_id(), b.url.clone());
        a.transport
            .set_identity(*b.mesh.agent_id(), b.signing.verifying_key().to_bytes());
        a.resolver
            .register_peer(&b.mesh.public_identity(), vec![b.url.clone()], NOW)
            .await;
    }

    #[tokio::test]
    async fn a_name_resolves_over_a_websocket() {
        let seeker = spawn_ws(1, false).await;
        let holder = spawn_ws(2, false).await;
        introduce(&seeker, &holder).await;

        let rec = record(1, &[]);
        holder.resolver.publish_local(rec.clone()).await.unwrap();

        let found = seeker.resolver.resolve(&rec.name).await.unwrap();
        assert_eq!(found, rec, "resolved over a real WebSocket upgrade");
        assert!(found.verify().is_ok());
        assert_eq!(seeker.transport.pooled_connections(), 1);
    }

    #[tokio::test]
    async fn a_name_resolves_over_an_encrypted_websocket() {
        let seeker = spawn_ws(3, true).await;
        let holder = spawn_ws(4, true).await;
        introduce(&seeker, &holder).await;
        assert!(seeker.transport.is_encrypted());

        let rec = record(1, &[]);
        holder.resolver.publish_local(rec.clone()).await.unwrap();
        assert_eq!(seeker.resolver.resolve(&rec.name).await.unwrap(), rec);
    }

    #[tokio::test]
    async fn a_capability_resolves_over_websockets_from_two_holders() {
        let seeker = spawn_ws(5, false).await;
        let a = spawn_ws(6, false).await;
        let b = spawn_ws(7, false).await;
        introduce(&seeker, &a).await;
        introduce(&seeker, &b).await;

        a.resolver
            .publish_local(record(1, &["web.search"]))
            .await
            .unwrap();
        b.resolver
            .publish_local(record(2, &["web.search"]))
            .await
            .unwrap();

        let providers = seeker.resolver.find_providers("web.search").await.unwrap();
        assert_eq!(providers.len(), 2);
        assert!(providers.iter().all(|p| p.verify().is_ok()));
    }

    #[tokio::test]
    async fn the_responder_answers_on_the_same_websocket() {
        // As with TCP: only the seeker knows how to reach the holder.
        let seeker = spawn_ws(8, false).await;
        let holder = spawn_ws(9, false).await;
        introduce(&seeker, &holder).await;
        assert_eq!(holder.transport.known_urls(), 0);

        let rec = record(1, &[]);
        holder.resolver.publish_local(rec.clone()).await.unwrap();
        assert_eq!(seeker.resolver.resolve(&rec.name).await.unwrap(), rec);
    }

    #[tokio::test]
    async fn a_connection_is_reused_across_lookups() {
        let seeker = spawn_ws(10, false).await;
        let holder = spawn_ws(11, false).await;
        introduce(&seeker, &holder).await;

        holder.resolver.publish_local(record(1, &[])).await.unwrap();
        holder.resolver.publish_local(record(2, &[])).await.unwrap();

        seeker.resolver.resolve(&record(1, &[]).name).await.unwrap();
        seeker.resolver.resolve(&record(2, &[]).name).await.unwrap();
        assert_eq!(
            seeker.transport.pooled_connections(),
            1,
            "a second lookup must not repeat the upgrade"
        );
    }

    #[tokio::test]
    async fn re_announcing_the_same_url_keeps_the_connection() {
        let seeker = spawn_ws(12, false).await;
        let holder = spawn_ws(13, false).await;
        introduce(&seeker, &holder).await;
        holder.resolver.publish_local(record(1, &[])).await.unwrap();
        seeker.resolver.resolve(&record(1, &[]).name).await.unwrap();
        assert_eq!(seeker.transport.pooled_connections(), 1);

        seeker
            .transport
            .set_url(*holder.mesh.agent_id(), holder.url.clone());
        assert_eq!(seeker.transport.pooled_connections(), 1);

        seeker
            .transport
            .set_url(*holder.mesh.agent_id(), "ws://127.0.0.1:1/");
        assert_eq!(seeker.transport.pooled_connections(), 0);
    }

    #[tokio::test]
    async fn sending_to_a_peer_with_no_url_fails_clearly() {
        let (transport, _rx) = WsNameTransport::new();
        let node = MeshNode::new(
            SigningIdentity::from_seed("lonely-ws", [200u8; 32]),
            MeshConfig::default(),
        );
        let envelope = node.name_resolve_request(
            *node.agent_id(),
            1,
            crate::naming::ResolveQuery::Name(SpineUri::did([1u8; 32])),
        );
        let err = transport.send(envelope).await.unwrap_err();
        assert!(err.to_string().contains("no URL known"), "{err}");
    }

    #[tokio::test]
    async fn an_unreachable_websocket_does_not_hang_the_lookup() {
        let seeker = spawn_ws(14, false).await;
        let ghost = spawn_ws(15, false).await;
        introduce(&seeker, &ghost).await;
        // Point at a port nothing is listening on.
        seeker
            .transport
            .set_url(*ghost.mesh.agent_id(), "ws://127.0.0.1:1/");

        let result = tokio::time::timeout(
            Duration::from_secs(10),
            seeker.resolver.resolve(&record(1, &[]).name),
        )
        .await
        .expect("resolve() hung on an unreachable WebSocket peer");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn an_announcement_propagates_over_websockets() {
        let publisher = spawn_ws(16, false).await;
        let receiver = spawn_ws(17, false).await;
        introduce(&publisher, &receiver).await;

        publisher
            .resolver
            .publish(record(1, &["web.search"]))
            .await
            .unwrap();

        for _ in 0..100 {
            if receiver.resolver.record_count().await > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(receiver.resolver.record_count().await, 1);
    }
}
