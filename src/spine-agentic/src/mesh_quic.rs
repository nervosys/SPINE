//! QUIC transport for the mesh.
//!
//! Unlike TCP and WebSocket, this is *not* the generic byte-pipe path. QUIC's
//! defining feature is stream multiplexing, and modelling a QUIC connection as
//! one long byte pipe would throw that away — which is precisely the thing worth
//! having here.
//!
//! ## One stream per exchange
//!
//! A DHT lookup fans out to α peers at once and then follows referrals, so a
//! node has several requests in flight continuously. Over a single TCP or
//! WebSocket connection those share one ordered byte stream: a slow or large
//! response delays every frame queued behind it, and head-of-line blocking is
//! exactly the pathology that makes a multi-hop lookup feel slow.
//!
//! So the mapping is:
//!
//! - **one QUIC connection per peer** — amortizing the handshake, and
//! - **one bidirectional stream per request/response exchange** — independently
//!   ordered, so concurrent lookups cannot block one another.
//!
//! A stream is opened, the request written, the response read from the same
//! stream, and the stream finished. The responder answers on the stream the
//! request arrived on, which keeps the same NAT-friendly property the other
//! transports have.
//!
//! ## Authentication
//!
//! QUIC brings TLS 1.3, so frames are confidential without further work. But
//! the endpoints here use self-signed certificates and a permissive verifier
//! (see `QuicEndpointBuilder`), so that TLS authenticates *nothing* about which
//! mesh node is on the other end.
//!
//! The mesh handshake fills that gap, run **once per connection** on the first
//! stream rather than per exchange: QUIC guarantees every stream belongs to the
//! same authenticated connection, so one handshake covers all of them. It is
//! used for identity only — the ML-KEM session keys are discarded, because
//! encrypting inside TLS would buy nothing but CPU.

use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

use ed25519_dalek::SigningKey;
use quinn::{Connection, Endpoint};
use spine_protocol::QuicEndpointBuilder;
use spine_transport::websocket::QuicStream;

use crate::mesh::MeshEnvelope;
use crate::mesh_tcp::{
    client_handshake, server_handshake, Inbound, ReplyPath, SocketTransport, INBOUND_QUEUE,
    MAX_FRAME_BYTES,
};
use crate::naming_mesh::{MeshNameResolver, NameMeshError, NameTransport};
use crate::AgentId;

use async_trait::async_trait;

/// How long to wait for a QUIC handshake before giving up on a peer.
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Install the process-wide rustls crypto provider, once.
///
/// rustls 0.23 requires an explicitly chosen provider and *panics* deep inside
/// the TLS stack if none is installed. Doing it here, lazily, means callers get
/// a working QUIC endpoint rather than a crash from a library three layers down
/// that never mentions the real cause. `ring` is the provider this workspace
/// already selects for rustls.
///
/// A failure means something else installed a provider first, which is fine —
/// that one is used.
pub fn install_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// A QUIC transport that opens one stream per exchange.
pub struct QuicNameTransport {
    endpoint: Endpoint,
    /// Where each peer can be reached.
    addresses: DashMap<AgentId, SocketAddr>,
    /// Expected mesh identity per peer, pinned during the connection handshake.
    identities: DashMap<AgentId, [u8; 32]>,
    /// Live connections, one per peer.
    connections: DashMap<AgentId, Connection>,
    /// Connections opened to bare seed addresses during bootstrap, keyed by
    /// endpoint because the peer behind them has no known identity yet.
    bootstrap: DashMap<String, Connection>,
    inbound: mpsc::Sender<Inbound>,
    /// Identity used to authenticate connections, when authentication is on.
    signing: Option<SigningKey>,
}

impl std::fmt::Debug for QuicNameTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuicNameTransport")
            .field("peers", &self.addresses.len())
            .field("connections", &self.connections.len())
            .field("authenticated", &self.signing.is_some())
            .finish_non_exhaustive()
    }
}

impl QuicNameTransport {
    /// A transport with no mesh-identity authentication.
    ///
    /// Frames are still confidential — QUIC's TLS handles that — but nothing
    /// verifies *which* mesh node answered. Envelopes remain individually
    /// signed, so records cannot be forged either way.
    pub fn new() -> Result<(Arc<Self>, mpsc::Receiver<Inbound>), NameMeshError> {
        Self::build(None)
    }

    /// A transport that authenticates every connection against mesh identity.
    pub fn authenticated(
        signing: SigningKey,
    ) -> Result<(Arc<Self>, mpsc::Receiver<Inbound>), NameMeshError> {
        Self::build(Some(signing))
    }

    fn build(
        signing: Option<SigningKey>,
    ) -> Result<(Arc<Self>, mpsc::Receiver<Inbound>), NameMeshError> {
        install_crypto_provider();
        let endpoint = QuicEndpointBuilder::new()
            .build_client()
            .map_err(|e| NameMeshError::Transport(format!("quic client endpoint: {e}")))?;
        let (tx, rx) = mpsc::channel(INBOUND_QUEUE);
        Ok((
            Arc::new(Self {
                endpoint,
                addresses: DashMap::new(),
                identities: DashMap::new(),
                connections: DashMap::new(),
                bootstrap: DashMap::new(),
                inbound: tx,
                signing,
            }),
            rx,
        ))
    }

    /// A sender for the same inbound stream, for a listener to share.
    pub fn inbound_sender(&self) -> mpsc::Sender<Inbound> {
        self.inbound.clone()
    }

    /// Record where a peer can be reached.
    pub fn set_address(&self, agent: AgentId, addr: SocketAddr) {
        let previous = self.addresses.insert(agent, addr);
        if previous != Some(addr) {
            self.connections.remove(&agent);
        }
    }

    /// Record the mesh identity expected at a peer.
    pub fn set_identity(&self, agent: AgentId, identity: [u8; 32]) {
        self.identities.insert(agent, identity);
    }

    pub fn known_addresses(&self) -> usize {
        self.addresses.len()
    }

    /// Live connections. Note this counts *connections*, not streams — many
    /// exchanges share one.
    pub fn open_connections(&self) -> usize {
        self.connections.len()
    }

    pub fn is_authenticated(&self) -> bool {
        self.signing.is_some()
    }

    pub fn evict(&self, agent: &AgentId) {
        self.connections.remove(agent);
    }

    /// Get or establish the connection to a peer.
    async fn connection(&self, agent: &AgentId) -> Result<Connection, NameMeshError> {
        if let Some(existing) = self.connections.get(agent) {
            // A closed connection is worse than none: it would fail every
            // subsequent stream open with a stale error.
            if existing.close_reason().is_none() {
                return Ok(existing.value().clone());
            }
        }
        self.connections.remove(agent);

        let addr = self
            .addresses
            .get(agent)
            .map(|e| *e.value())
            .ok_or_else(|| NameMeshError::Transport(format!("no address known for {agent:?}")))?;

        let connecting = self
            .endpoint
            .connect(addr, "localhost")
            .map_err(|e| NameMeshError::Transport(format!("quic connect {addr}: {e}")))?;

        // Bound the handshake ourselves. Quinn's own timeout is tuned for
        // long-lived connections and far exceeds any lookup budget, so an
        // address with nothing listening would otherwise stall a wave well past
        // the point the resolver had given up on it.
        let connection = tokio::time::timeout(CONNECT_TIMEOUT, connecting)
            .await
            .map_err(|_| NameMeshError::Transport(format!("quic handshake to {addr} timed out")))?
            .map_err(|e| NameMeshError::Transport(format!("quic handshake {addr}: {e}")))?;

        // Authenticate the connection once, on its first stream.
        if let Some(signing) = &self.signing {
            let (send, recv) = connection
                .open_bi()
                .await
                .map_err(|e| NameMeshError::Transport(format!("open control stream: {e}")))?;
            let mut stream = QuicStream::new(send, recv);
            let expected = self.identities.get(agent).map(|e| *e.value());
            // The derived session keys are dropped: QUIC already encrypts, and
            // this exchange exists only to prove who is on the far end.
            let _session = client_handshake(&mut stream, signing, expected.as_ref()).await?;
        }

        self.connections.insert(*agent, connection.clone());
        Ok(connection)
    }

    /// Send one envelope on its own stream, then read the reply on the same one.
    async fn exchange(
        &self,
        target: AgentId,
        envelope: &MeshEnvelope,
    ) -> Result<(), NameMeshError> {
        let connection = self.connection(&target).await?;
        let (send, recv) = match connection.open_bi().await {
            Ok(pair) => pair,
            Err(_) => {
                // The pooled connection died. One transparent retry.
                self.connections.remove(&target);
                let connection = self.connection(&target).await?;
                connection
                    .open_bi()
                    .await
                    .map_err(|e| NameMeshError::Transport(format!("open stream: {e}")))?
            }
        };

        let mut stream = QuicStream::new(send, recv);
        crate::mesh_tcp::write_envelope(&mut stream, envelope).await?;

        // Read the response on this stream in the background, so the caller is
        // not blocked and other exchanges proceed on their own streams.
        let inbound = self.inbound.clone();
        tokio::spawn(async move {
            match crate::mesh_tcp::read_envelope(&mut stream).await {
                Ok(Some(reply)) => {
                    // A response needs no reply path of its own — but the type
                    // requires one, and answering back down the same stream is
                    // the correct behaviour if the peer ever asks something.
                    let path = ReplyPath::Plain(SocketTransport::from_writer(stream));
                    let _ = inbound
                        .send(Inbound {
                            envelope: reply,
                            reply: path,
                        })
                        .await;
                }
                Ok(None) => {}
                Err(e) => tracing::debug!("quic exchange ended: {e}"),
            }
        });
        Ok(())
    }

    /// Connect to a seed address whose mesh identity is not yet known, and run
    /// one exchange on it.
    ///
    /// Cached by endpoint rather than by agent id, because there is no agent id
    /// yet — establishing one is what the exchange is for. When the transport is
    /// authenticated the handshake still runs, but unpinned: it proves the peer
    /// holds *some* key, and the ack's signature is what binds that key to an
    /// identity we then trust.
    async fn provisional(&self, endpoint: &str) -> Result<Connection, NameMeshError> {
        if let Some(existing) = self.bootstrap.get(endpoint) {
            if existing.close_reason().is_none() {
                return Ok(existing.value().clone());
            }
        }
        self.bootstrap.remove(endpoint);

        let addr: SocketAddr = endpoint
            .parse()
            .map_err(|e| NameMeshError::Transport(format!("bad seed address `{endpoint}`: {e}")))?;

        let connecting = self
            .endpoint
            .connect(addr, "localhost")
            .map_err(|e| NameMeshError::Transport(format!("quic connect seed {addr}: {e}")))?;
        let connection = tokio::time::timeout(CONNECT_TIMEOUT, connecting)
            .await
            .map_err(|_| {
                NameMeshError::Transport(format!("quic handshake to seed {addr} timed out"))
            })?
            .map_err(|e| NameMeshError::Transport(format!("quic handshake {addr}: {e}")))?;

        if let Some(signing) = &self.signing {
            let (send, recv) = connection
                .open_bi()
                .await
                .map_err(|e| NameMeshError::Transport(format!("open control stream: {e}")))?;
            let mut stream = QuicStream::new(send, recv);
            let _session = client_handshake(&mut stream, signing, None).await?;
        }

        self.bootstrap
            .insert(endpoint.to_string(), connection.clone());
        Ok(connection)
    }

    /// Run one exchange over a provisional connection.
    async fn provisional_exchange(
        &self,
        endpoint: &str,
        envelope: &MeshEnvelope,
    ) -> Result<(), NameMeshError> {
        let connection = self.provisional(endpoint).await?;
        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(|e| NameMeshError::Transport(format!("open seed stream: {e}")))?;

        let mut stream = QuicStream::new(send, recv);
        crate::mesh_tcp::write_envelope(&mut stream, envelope).await?;

        let inbound = self.inbound.clone();
        tokio::spawn(async move {
            match crate::mesh_tcp::read_envelope(&mut stream).await {
                Ok(Some(reply)) => {
                    let path = ReplyPath::Plain(SocketTransport::from_writer(stream));
                    let _ = inbound
                        .send(Inbound {
                            envelope: reply,
                            reply: path,
                        })
                        .await;
                }
                Ok(None) => {}
                Err(e) => tracing::debug!("quic bootstrap exchange ended: {e}"),
            }
        });
        Ok(())
    }
}

#[async_trait]
impl NameTransport for QuicNameTransport {
    async fn send(&self, envelope: MeshEnvelope) -> Result<(), NameMeshError> {
        match envelope.to {
            crate::mesh::MeshTarget::Agent(id) => self.exchange(id, &envelope).await,
            _ => {
                let peers: Vec<AgentId> = self.addresses.iter().map(|e| *e.key()).collect();
                for peer in peers {
                    let mut addressed = envelope.clone();
                    addressed.to = crate::mesh::MeshTarget::Agent(peer);
                    let _ = self.exchange(peer, &addressed).await;
                }
                Ok(())
            }
        }
    }

    async fn send_to(&self, endpoint: &str, envelope: MeshEnvelope) -> Result<(), NameMeshError> {
        self.provisional_exchange(endpoint, &envelope).await
    }

    /// Take the first endpoint that parses as a socket address, ignoring URLs
    /// meant for another transport.
    async fn learn(&self, agent: AgentId, endpoints: &[String]) {
        if let Some(addr) = endpoints.iter().find_map(|e| e.parse::<SocketAddr>().ok()) {
            self.set_address(agent, addr);
        }
    }

    async fn release(&self, endpoint: &str) {
        self.bootstrap.remove(endpoint);
    }
}

/// Accepts QUIC connections and serves each stream as one exchange.
pub struct QuicListener {
    endpoint: Endpoint,
}

impl std::fmt::Debug for QuicListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuicListener").finish_non_exhaustive()
    }
}

impl QuicListener {
    /// Bind a QUIC server endpoint. Pass port 0 to let the OS choose.
    pub fn bind(addr: SocketAddr) -> Result<Self, NameMeshError> {
        install_crypto_provider();
        let endpoint = QuicEndpointBuilder::new()
            .build_server(addr)
            .map_err(|e| NameMeshError::Transport(format!("quic bind {addr}: {e}")))?;
        Ok(Self { endpoint })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, NameMeshError> {
        self.endpoint
            .local_addr()
            .map_err(|e| NameMeshError::Transport(format!("local_addr: {e}")))
    }

    /// Accept connections until the task is dropped.
    pub async fn serve(self, inbound: mpsc::Sender<Inbound>, signing: Option<SigningKey>) {
        while let Some(incoming) = self.endpoint.accept().await {
            let inbound = inbound.clone();
            let signing = signing.clone();
            tokio::spawn(async move {
                match incoming.await {
                    Ok(connection) => serve_connection(connection, inbound, signing).await,
                    Err(e) => tracing::debug!("quic connection failed: {e}"),
                }
            });
        }
    }
}

/// Serve every stream on one accepted connection.
async fn serve_connection(
    connection: Connection,
    inbound: mpsc::Sender<Inbound>,
    signing: Option<SigningKey>,
) {
    // If authentication is on, the first stream is the handshake and carries no
    // envelope. Every stream after it belongs to the same proven connection.
    if let Some(key) = &signing {
        match connection.accept_bi().await {
            Ok((send, recv)) => {
                let mut stream = QuicStream::new(send, recv);
                if let Err(e) = server_handshake(&mut stream, key).await {
                    tracing::debug!("quic peer failed authentication: {e}");
                    return;
                }
            }
            Err(e) => {
                tracing::debug!("quic control stream never arrived: {e}");
                return;
            }
        }
    }

    loop {
        let (send, recv) = match connection.accept_bi().await {
            Ok(pair) => pair,
            Err(_) => return, // connection closed
        };
        let inbound = inbound.clone();
        tokio::spawn(async move {
            let mut stream = QuicStream::new(send, recv);
            match crate::mesh_tcp::read_envelope(&mut stream).await {
                Ok(Some(envelope)) => {
                    let path = ReplyPath::Plain(SocketTransport::from_writer(stream));
                    let _ = inbound
                        .send(Inbound {
                            envelope,
                            reply: path,
                        })
                        .await;
                }
                Ok(None) => {}
                Err(e) => tracing::debug!("quic stream dropped: {e}"),
            }
        });
    }
}

/// Bind a QUIC listener, wire it to `resolver`, and start serving.
pub async fn serve_quic_node(
    bind_addr: SocketAddr,
    resolver: Arc<MeshNameResolver>,
    transport: Arc<QuicNameTransport>,
    inbound: mpsc::Receiver<Inbound>,
    signing: Option<SigningKey>,
) -> Result<SocketAddr, NameMeshError> {
    let listener = QuicListener::bind(bind_addr)?;
    let addr = listener.local_addr()?;
    tokio::spawn(listener.serve(transport.inbound_sender(), signing));
    tokio::spawn(crate::mesh_tcp::pump(inbound, resolver));
    Ok(addr)
}

/// Largest envelope this transport will accept, mirroring the others.
pub const MAX_QUIC_FRAME: usize = MAX_FRAME_BYTES;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::SigningIdentity;
    use crate::mesh::{MeshConfig, MeshNode};
    use spine_name::{Endpoint as NameEndpoint, NameRecord, SpineUri};
    use std::time::Duration;

    const NOW: u64 = 1_700_000_000;

    fn record(seed: u8, caps: &[&str]) -> NameRecord {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name, 1, NOW)
            .unwrap()
            .with_ttl(3600)
            .with_endpoint(NameEndpoint::new("quic", format!("10.0.0.{seed}:9440")));
        for c in caps {
            rec = rec.with_capability(*c);
        }
        rec.sign(&key).unwrap();
        rec
    }

    struct QuicNode {
        resolver: Arc<MeshNameResolver>,
        transport: Arc<QuicNameTransport>,
        mesh: Arc<MeshNode>,
        addr: SocketAddr,
        signing: SigningKey,
    }

    async fn spawn_quic(seed: u8, authenticated: bool) -> QuicNode {
        let identity = SigningIdentity::from_seed(&format!("quic{seed}"), [90 + seed; 32]);
        let signing = SigningKey::from_bytes(&[90 + seed; 32]);
        let mesh = Arc::new(MeshNode::new(identity, MeshConfig::default()));

        let (transport, inbound) = if authenticated {
            QuicNameTransport::authenticated(signing.clone()).unwrap()
        } else {
            QuicNameTransport::new().unwrap()
        };
        let resolver = Arc::new(
            MeshNameResolver::new(mesh.clone(), transport.clone())
                .with_clock(|| NOW)
                .with_timeout(Duration::from_secs(10)),
        );
        let addr = serve_quic_node(
            "127.0.0.1:0".parse().unwrap(),
            resolver.clone(),
            transport.clone(),
            inbound,
            authenticated.then(|| signing.clone()),
        )
        .await
        .unwrap();

        QuicNode {
            resolver,
            transport,
            mesh,
            addr,
            signing,
        }
    }

    async fn introduce(a: &QuicNode, b: &QuicNode) {
        a.transport.set_address(*b.mesh.agent_id(), b.addr);
        a.transport
            .set_identity(*b.mesh.agent_id(), b.signing.verifying_key().to_bytes());
        a.resolver
            .register_peer(&b.mesh.public_identity(), vec![b.addr.to_string()], NOW)
            .await;
    }

    #[tokio::test]
    async fn a_name_resolves_over_quic() {
        let seeker = spawn_quic(1, false).await;
        let holder = spawn_quic(2, false).await;
        introduce(&seeker, &holder).await;

        let rec = record(1, &[]);
        holder.resolver.publish_local(rec.clone()).await.unwrap();

        let found = seeker.resolver.resolve(&rec.name).await.unwrap();
        assert_eq!(found, rec, "resolved over a real QUIC connection");
        assert!(found.verify().is_ok());
    }

    #[tokio::test]
    async fn a_name_resolves_over_an_authenticated_quic_connection() {
        let seeker = spawn_quic(3, true).await;
        let holder = spawn_quic(4, true).await;
        introduce(&seeker, &holder).await;
        assert!(seeker.transport.is_authenticated());

        let rec = record(1, &[]);
        holder.resolver.publish_local(rec.clone()).await.unwrap();
        assert_eq!(seeker.resolver.resolve(&rec.name).await.unwrap(), rec);
    }

    #[tokio::test]
    async fn a_capability_resolves_over_quic_from_two_holders() {
        let seeker = spawn_quic(5, false).await;
        let a = spawn_quic(6, false).await;
        let b = spawn_quic(7, false).await;
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
    async fn many_exchanges_share_one_connection() {
        // The property the whole design rests on: streams multiplex, so a
        // second lookup does not mean a second handshake.
        let seeker = spawn_quic(8, false).await;
        let holder = spawn_quic(9, false).await;
        introduce(&seeker, &holder).await;

        for seed in 1..=5u8 {
            holder
                .resolver
                .publish_local(record(seed, &[]))
                .await
                .unwrap();
        }
        for seed in 1..=5u8 {
            seeker
                .resolver
                .resolve(&record(seed, &[]).name)
                .await
                .unwrap();
        }
        assert_eq!(
            seeker.transport.open_connections(),
            1,
            "five exchanges must share one QUIC connection"
        );
    }

    #[tokio::test]
    async fn concurrent_lookups_do_not_block_one_another() {
        let seeker = spawn_quic(10, false).await;
        let holder = spawn_quic(11, false).await;
        introduce(&seeker, &holder).await;
        for seed in 1..=4u8 {
            holder
                .resolver
                .publish_local(record(seed, &[]))
                .await
                .unwrap();
        }

        // Four lookups issued at once, each on its own stream.
        let results = futures::future::join_all((1..=4u8).map(|seed| {
            let r = seeker.resolver.clone();
            async move { r.resolve(&record(seed, &[]).name).await }
        }))
        .await;

        assert!(results.iter().all(|r| r.is_ok()), "{results:?}");
        assert_eq!(seeker.transport.open_connections(), 1);
    }

    #[tokio::test]
    async fn an_announcement_propagates_over_quic() {
        let publisher = spawn_quic(12, false).await;
        let receiver = spawn_quic(13, false).await;
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

    #[tokio::test]
    async fn sending_to_a_peer_with_no_address_fails_clearly() {
        let (transport, _rx) = QuicNameTransport::new().unwrap();
        let node = MeshNode::new(
            SigningIdentity::from_seed("lonely-quic", [201u8; 32]),
            MeshConfig::default(),
        );
        let envelope = node.name_resolve_request(
            *node.agent_id(),
            1,
            crate::naming::ResolveQuery::Name(SpineUri::did([1u8; 32])),
        );
        let err = transport.send(envelope).await.unwrap_err();
        assert!(err.to_string().contains("no address known"), "{err}");
    }

    #[tokio::test]
    async fn an_unreachable_peer_does_not_hang_the_lookup() {
        let seeker = spawn_quic(14, false).await;
        let ghost = spawn_quic(15, false).await;
        introduce(&seeker, &ghost).await;
        seeker
            .transport
            .set_address(*ghost.mesh.agent_id(), "127.0.0.1:1".parse().unwrap());

        let result = tokio::time::timeout(
            Duration::from_secs(20),
            seeker.resolver.resolve(&record(1, &[]).name),
        )
        .await
        .expect("resolve() hung on an unreachable QUIC peer");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn re_announcing_the_same_address_keeps_the_connection() {
        let seeker = spawn_quic(16, false).await;
        let holder = spawn_quic(17, false).await;
        introduce(&seeker, &holder).await;
        holder.resolver.publish_local(record(1, &[])).await.unwrap();
        seeker.resolver.resolve(&record(1, &[]).name).await.unwrap();
        assert_eq!(seeker.transport.open_connections(), 1);

        seeker
            .transport
            .set_address(*holder.mesh.agent_id(), holder.addr);
        assert_eq!(seeker.transport.open_connections(), 1);

        seeker
            .transport
            .set_address(*holder.mesh.agent_id(), "127.0.0.1:1".parse().unwrap());
        assert_eq!(seeker.transport.open_connections(), 0);
    }
}
