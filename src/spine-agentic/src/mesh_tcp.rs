//! A TCP socket layer for the mesh — the piece that puts envelopes on a wire.
//!
//! `MeshNode` builds and signs envelopes but has never owned a socket, and until
//! now nothing in the workspace transmitted one: the mesh was a complete routing
//! and gossip design with no transport under it. This module supplies the
//! missing half, and with it [`NameTransport`] gains a real implementation, so
//! name resolution runs between separate processes rather than only inside a
//! test harness.
//!
//! ## Every connection is bidirectional
//!
//! The design decision that matters here: a request is answered on the socket it
//! arrived on, never by dialing the asker back. Routing replies through an
//! address book would mean every responder had to be separately introduced to
//! every asker before it could answer — a bootstrapping problem in the general
//! case, and simply impossible through NAT, where the asker has no dialable
//! address.
//!
//! So both directions are symmetric. Whether a connection was dialed or
//! accepted, it gets a reader task feeding the shared inbound channel and a
//! write half usable as a [`SocketTransport`] for replies. The only asymmetry
//! left is who opened it.
//!
//! ## Framing
//!
//! A 4-byte big-endian length prefix followed by a JSON-serialized
//! [`MeshEnvelope`]. Length-prefixed rather than delimiter-scanned because an
//! envelope carries signatures and arbitrary metadata — any delimiter would need
//! escaping, and an escaping bug in a security-carrying frame is a poor trade
//! for four bytes. [`MAX_FRAME_BYTES`] bounds an allocation *before* it is made,
//! so a peer cannot induce an out-of-memory abort by announcing a huge frame.
//!
//! ## Connection reuse
//!
//! Dialing per message would put a TCP handshake in front of every DHT hop, and
//! a lookup is several hops deep. Connections are pooled per peer and re-dialed
//! once on write failure — the common case being a peer that restarted, where a
//! single transparent retry is the difference between a stalled lookup and an
//! invisible reconnect.

use async_trait::async_trait;
use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};

use ed25519_dalek::SigningKey;
use spine_crypto::handshake::{Initiator, Responder, Session, MAX_HANDSHAKE_BYTES};

use crate::mesh::MeshEnvelope;
use crate::naming_mesh::{MeshNameResolver, NameMeshError, NameTransport};
use crate::AgentId;

/// Largest envelope accepted, in bytes. Generous for a record with links and
/// metadata; far below anything that could exhaust memory.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

/// Depth of the inbound queue before backpressure applies.
pub const INBOUND_QUEUE: usize = 1024;

// ───────────────────────────────── Framing ─────────────────────────────────

/// Write a length-prefixed envelope.
pub async fn write_envelope<W>(w: &mut W, envelope: &MeshEnvelope) -> Result<(), NameMeshError>
where
    W: AsyncWriteExt + Unpin,
{
    let body = serde_json::to_vec(envelope)
        .map_err(|e| NameMeshError::Transport(format!("encode: {e}")))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(NameMeshError::Transport(format!(
            "envelope of {} bytes exceeds the {MAX_FRAME_BYTES}-byte limit",
            body.len()
        )));
    }
    // One buffer, one write: a split header/body write can interleave with a
    // concurrent writer on the same socket and corrupt the stream.
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);
    w.write_all(&frame)
        .await
        .map_err(|e| NameMeshError::Transport(format!("write: {e}")))?;
    w.flush()
        .await
        .map_err(|e| NameMeshError::Transport(format!("flush: {e}")))
}

/// Read one length-prefixed envelope.
///
/// `Ok(None)` is a clean end-of-stream — an ordinary disconnect, not an error.
pub async fn read_envelope<R>(r: &mut R) -> Result<Option<MeshEnvelope>, NameMeshError>
where
    R: AsyncReadExt + Unpin,
{
    let mut len_bytes = [0u8; 4];
    match r.read_exact(&mut len_bytes).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(NameMeshError::Transport(format!("read length: {e}"))),
    }

    let len = u32::from_be_bytes(len_bytes) as usize;
    // Checked before allocating, so the announced size cannot itself be the attack.
    if len > MAX_FRAME_BYTES {
        return Err(NameMeshError::Transport(format!(
            "peer announced a {len}-byte frame, over the {MAX_FRAME_BYTES}-byte limit"
        )));
    }
    if len == 0 {
        return Err(NameMeshError::Transport("zero-length frame".into()));
    }

    let mut body = vec![0u8; len];
    r.read_exact(&mut body)
        .await
        .map_err(|e| NameMeshError::Transport(format!("read body: {e}")))?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| NameMeshError::Transport(format!("decode: {e}")))
}

// ──────────────────────────── Per-socket transport ──────────────────────────

/// A [`NameTransport`] bound to one socket.
///
/// It ignores the envelope's addressee and writes to its own connection, which
/// is exactly what answering a request requires.
#[derive(Clone)]
pub struct SocketTransport {
    writer: Arc<Mutex<Box<dyn AsyncWrite + Send + Unpin>>>,
}

impl std::fmt::Debug for SocketTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SocketTransport").finish_non_exhaustive()
    }
}

impl SocketTransport {
    /// Wrap any writer as a reply path. Used by transports (such as QUIC) that
    /// manage their own streams rather than going through `attach`.
    pub fn from_writer(writer: impl AsyncWrite + Send + Unpin + 'static) -> Self {
        Self::new(writer)
    }

    fn new(writer: impl AsyncWrite + Send + Unpin + 'static) -> Self {
        Self {
            writer: Arc::new(Mutex::new(Box::new(writer))),
        }
    }
}

#[async_trait]
impl NameTransport for SocketTransport {
    async fn send(&self, envelope: MeshEnvelope) -> Result<(), NameMeshError> {
        let mut guard = self.writer.lock().await;
        write_envelope(&mut *guard, &envelope).await
    }
}

/// How to answer an envelope: on the connection it arrived on, in whichever
/// mode that connection uses.
#[derive(Debug, Clone)]
pub enum ReplyPath {
    Plain(SocketTransport),
    Secure(SecureSocketTransport),
}

#[async_trait]
impl NameTransport for ReplyPath {
    async fn send(&self, envelope: MeshEnvelope) -> Result<(), NameMeshError> {
        match self {
            ReplyPath::Plain(t) => t.send(envelope).await,
            ReplyPath::Secure(t) => t.send(envelope).await,
        }
    }
}

/// An envelope that arrived, together with the way to answer it.
#[derive(Debug)]
pub struct Inbound {
    pub envelope: MeshEnvelope,
    /// Write back on the connection it came from.
    pub reply: ReplyPath,
}

/// Read envelopes off one connection until it closes, forwarding each.
///
/// A framing error closes the connection rather than being tolerated: after one,
/// the stream position is unknown and continuing would be parsing garbage.
async fn read_loop<R: AsyncRead + Unpin>(
    mut reader: R,
    reply: SocketTransport,
    inbound: mpsc::Sender<Inbound>,
) {
    loop {
        match read_envelope(&mut reader).await {
            Ok(Some(envelope)) => {
                let item = Inbound {
                    envelope,
                    reply: ReplyPath::Plain(reply.clone()),
                };
                if inbound.send(item).await.is_err() {
                    return; // pump is gone; nothing left to do
                }
            }
            Ok(None) => return, // clean disconnect
            Err(e) => {
                tracing::debug!("mesh connection closed after a framing error: {e}");
                return;
            }
        }
    }
}

/// Wire up a connection in both directions: pool its writer, read from it.
///
/// Generic over the stream so TCP, WebSocket, and QUIC all reuse one path —
/// the framing, the reply plumbing, and the encryption are transport-agnostic,
/// and only the way a connection is established differs.
pub fn attach<S>(stream: S, inbound: mpsc::Sender<Inbound>) -> SocketTransport
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (reader, writer) = tokio::io::split(stream);
    let transport = SocketTransport::new(writer);
    tokio::spawn(read_loop(reader, transport.clone(), inbound));
    transport
}

// ─────────────────────────── Encrypted connections ──────────────────────────

/// Perform the dialing half of a [`spine_crypto::handshake`] over a socket.
///
/// `expected_peer` pins the far end to the mesh identity we meant to reach.
/// Passing `None` is the bootstrap case: dialing an address whose identity is
/// genuinely not yet known.
pub async fn client_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    signing: &SigningKey,
    expected_peer: Option<&[u8; 32]>,
) -> Result<Session, NameMeshError> {
    let (initiator, hello) = Initiator::start(signing);
    write_frame(stream, &hello).await?;
    let reply = read_frame(stream, MAX_HANDSHAKE_BYTES)
        .await?
        .ok_or_else(|| NameMeshError::Transport("peer closed during handshake".into()))?;
    initiator
        .finish(&reply, expected_peer)
        .map_err(|e| NameMeshError::Transport(format!("handshake: {e}")))
}

/// Perform the accepting half of a handshake over a socket.
pub async fn server_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    signing: &SigningKey,
) -> Result<Session, NameMeshError> {
    let hello = read_frame(stream, MAX_HANDSHAKE_BYTES)
        .await?
        .ok_or_else(|| NameMeshError::Transport("peer closed during handshake".into()))?;
    let accepted = Responder::accept(signing, &hello)
        .map_err(|e| NameMeshError::Transport(format!("handshake: {e}")))?;
    write_frame(stream, &accepted.reply).await?;
    Ok(accepted.session)
}

/// Write raw length-prefixed bytes — used for handshake messages, which are not
/// yet envelopes and are not yet encrypted.
async fn write_frame<W>(w: &mut W, bytes: &[u8]) -> Result<(), NameMeshError>
where
    W: AsyncWriteExt + Unpin,
{
    let mut frame = Vec::with_capacity(4 + bytes.len());
    frame.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    frame.extend_from_slice(bytes);
    w.write_all(&frame)
        .await
        .map_err(|e| NameMeshError::Transport(format!("write: {e}")))?;
    w.flush()
        .await
        .map_err(|e| NameMeshError::Transport(format!("flush: {e}")))
}

/// Read raw length-prefixed bytes, bounded by `max`.
async fn read_frame<R>(r: &mut R, max: usize) -> Result<Option<Vec<u8>>, NameMeshError>
where
    R: AsyncReadExt + Unpin,
{
    let mut len_bytes = [0u8; 4];
    match r.read_exact(&mut len_bytes).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(NameMeshError::Transport(format!("read length: {e}"))),
    }
    let len = u32::from_be_bytes(len_bytes) as usize;
    if len > max || len == 0 {
        return Err(NameMeshError::Transport(format!(
            "frame of {len} bytes is outside the accepted range (1..={max})"
        )));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)
        .await
        .map_err(|e| NameMeshError::Transport(format!("read body: {e}")))?;
    Ok(Some(body))
}

/// A [`NameTransport`] over an encrypted session.
///
/// The session holds per-direction AEAD counters, so sealing must be serialized;
/// the mutex is what guarantees two concurrent sends can never be handed the
/// same nonce.
#[derive(Clone)]
pub struct SecureSocketTransport {
    inner: Arc<Mutex<SecureWriter>>,
}

struct SecureWriter {
    writer: Box<dyn AsyncWrite + Send + Unpin>,
    session: Arc<Mutex<Session>>,
}

impl std::fmt::Debug for SecureSocketTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecureSocketTransport")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl NameTransport for SecureSocketTransport {
    async fn send(&self, envelope: MeshEnvelope) -> Result<(), NameMeshError> {
        let body = serde_json::to_vec(&envelope)
            .map_err(|e| NameMeshError::Transport(format!("encode: {e}")))?;
        if body.len() > MAX_FRAME_BYTES {
            return Err(NameMeshError::Transport(format!(
                "envelope of {} bytes exceeds the {MAX_FRAME_BYTES}-byte limit",
                body.len()
            )));
        }
        let mut guard = self.inner.lock().await;
        // Seal and write under one lock: releasing between them would let a
        // second sender interleave and deliver frames out of counter order,
        // which the receiver correctly refuses.
        let sealed = {
            let mut session = guard.session.lock().await;
            session.seal(&body)
        };
        write_frame(&mut guard.writer, &sealed).await
    }
}

/// Read encrypted envelopes off one connection until it closes.
async fn secure_read_loop<R: AsyncRead + Unpin>(
    mut reader: R,
    session: Arc<Mutex<Session>>,
    reply: SecureSocketTransport,
    inbound: mpsc::Sender<Inbound>,
) {
    loop {
        let frame = match read_frame(&mut reader, MAX_FRAME_BYTES + 1024).await {
            Ok(Some(f)) => f,
            Ok(None) => return,
            Err(e) => {
                tracing::debug!("secure mesh connection closed: {e}");
                return;
            }
        };
        let plaintext = {
            let mut s = session.lock().await;
            match s.open(&frame) {
                Ok(p) => p,
                Err(e) => {
                    // An authentication failure means tampering, replay, or a
                    // desynchronized stream. None is recoverable — the session
                    // keys and counters are no longer trustworthy.
                    tracing::warn!("dropping secure mesh connection: {e}");
                    return;
                }
            }
        };
        let envelope: MeshEnvelope = match serde_json::from_slice(&plaintext) {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!("undecodable envelope on a secure connection: {e}");
                return;
            }
        };
        let item = Inbound {
            envelope,
            reply: ReplyPath::Secure(reply.clone()),
        };
        if inbound.send(item).await.is_err() {
            return;
        }
    }
}

/// Wire up an encrypted connection in both directions.
pub fn attach_secure<S>(
    stream: S,
    session: Session,
    inbound: mpsc::Sender<Inbound>,
) -> SecureSocketTransport
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (reader, writer) = tokio::io::split(stream);
    let session = Arc::new(Mutex::new(session));
    let transport = SecureSocketTransport {
        inner: Arc::new(Mutex::new(SecureWriter {
            writer: Box::new(writer),
            session: session.clone(),
        })),
    };
    tokio::spawn(secure_read_loop(
        reader,
        session,
        transport.clone(),
        inbound,
    ));
    transport
}

// ──────────────────────────────── Transport ────────────────────────────────

/// A pooled TCP transport for mesh envelopes.
#[derive(Debug)]
pub struct TcpNameTransport {
    /// Where each agent can be reached.
    addresses: DashMap<AgentId, SocketAddr>,
    /// Live outbound connections, one per peer.
    pool: DashMap<AgentId, ReplyPath>,
    /// Connections opened to bare addresses during bootstrap, keyed by endpoint
    /// because the peer behind them has no known identity yet.
    bootstrap: DashMap<String, ReplyPath>,
    /// Expected mesh identity per peer, used to pin the handshake.
    identities: DashMap<AgentId, [u8; 32]>,
    /// Everything read off any connection this transport opened.
    inbound: mpsc::Sender<Inbound>,
    /// Whether connections are encrypted.
    security: Security,
}

/// How a transport protects its connections.
///
/// Plaintext is offered because mesh envelopes are independently signed, so
/// records cannot be forged either way — but it leaks *what an agent is looking
/// for*, which is usually a direct read on the agent's task. Encrypted is the
/// right default for anything crossing a network you do not own.
pub enum Security {
    /// No transport encryption. Signed envelopes only.
    Plaintext,
    /// ML-KEM-768 + Ed25519 handshake, AES-256-GCM frames.
    ///
    /// Boxed because a `SigningKey` dwarfs the empty `Plaintext` variant, and an
    /// un-boxed variant would size every `Security` — including a plaintext
    /// one — for the key it does not hold.
    Encrypted(Box<EncryptionConfig>),
}

/// Keys and parameters for an encrypted transport.
pub struct EncryptionConfig {
    /// The node's long-term mesh identity — the same Ed25519 key that places it
    /// in the DHT keyspace and signs its records.
    pub signing: SigningKey,
}

impl std::fmt::Debug for Security {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Security::Plaintext => f.write_str("Plaintext"),
            Security::Encrypted(_) => f.write_str("Encrypted"),
        }
    }
}

impl TcpNameTransport {
    /// Build a transport and the inbound stream its connections feed.
    ///
    /// The two come as a pair because a dialed connection is bidirectional: a
    /// response arrives on the socket the request went out on, so the transport
    /// must have somewhere to put it. Hand the receiver to [`pump`].
    pub fn new() -> (Arc<Self>, mpsc::Receiver<Inbound>) {
        Self::with_security(Security::Plaintext)
    }

    /// Build an encrypted transport keyed to this node's mesh identity.
    ///
    /// Every connection it opens performs an authenticated, forward-secret
    /// handshake, and pins the far end to the identity recorded for that peer.
    pub fn encrypted(signing: SigningKey) -> (Arc<Self>, mpsc::Receiver<Inbound>) {
        Self::with_security(Security::Encrypted(Box::new(EncryptionConfig { signing })))
    }

    /// Build a transport with an explicit security mode.
    pub fn with_security(security: Security) -> (Arc<Self>, mpsc::Receiver<Inbound>) {
        let (tx, rx) = mpsc::channel(INBOUND_QUEUE);
        let transport = Arc::new(Self {
            addresses: DashMap::new(),
            pool: DashMap::new(),
            bootstrap: DashMap::new(),
            identities: DashMap::new(),
            inbound: tx,
            security,
        });
        (transport, rx)
    }

    /// Whether connections are encrypted.
    pub fn is_encrypted(&self) -> bool {
        matches!(self.security, Security::Encrypted(_))
    }

    /// Record the mesh identity expected at a peer, so the handshake can pin it.
    ///
    /// Without this, a dial is unpinned: it still gets confidentiality and knows
    /// *who* answered, but cannot refuse an unexpected peer up front.
    pub fn set_identity(&self, agent: AgentId, identity: [u8; 32]) {
        self.identities.insert(agent, identity);
    }

    /// A sender for the same inbound stream, for a listener to share.
    pub fn inbound_sender(&self) -> mpsc::Sender<Inbound> {
        self.inbound.clone()
    }

    /// Record where a peer can be reached.
    pub fn set_address(&self, agent: AgentId, addr: SocketAddr) {
        let previous = self.addresses.insert(agent, addr);
        // A genuinely changed address invalidates the pooled connection to the
        // old one; re-announcing the same address leaves it alone.
        if previous != Some(addr) {
            self.pool.remove(&agent);
        }
    }

    /// Peers with a known address.
    pub fn known_addresses(&self) -> usize {
        self.addresses.len()
    }

    /// Currently pooled connections.
    pub fn pooled_connections(&self) -> usize {
        self.pool.len()
    }

    /// Drop a peer's pooled connection, forcing a re-dial next time.
    pub fn evict(&self, agent: &AgentId) {
        self.pool.remove(agent);
    }

    async fn connection(&self, agent: &AgentId) -> Result<ReplyPath, NameMeshError> {
        if let Some(existing) = self.pool.get(agent) {
            return Ok(existing.value().clone());
        }
        let addr = self
            .addresses
            .get(agent)
            .map(|e| *e.value())
            .ok_or_else(|| NameMeshError::Transport(format!("no address known for {agent:?}")))?;

        let mut stream = TcpStream::connect(addr)
            .await
            .map_err(|e| NameMeshError::Transport(format!("dial {addr}: {e}")))?;
        // Envelopes are small and latency-sensitive; Nagle would add up to a
        // 40 ms delay per DHT hop for no benefit.
        let _ = stream.set_nodelay(true);

        let transport = match &self.security {
            Security::Plaintext => ReplyPath::Plain(attach(stream, self.inbound.clone())),
            Security::Encrypted(cfg) => {
                let signing = &cfg.signing;
                // Pin the far end to the identity we meant to reach, when we
                // know it. The keyspace id and the handshake key are the same
                // Ed25519 key, so "the peer I dialed" and "the peer whose
                // records I would trust" are one fact.
                let expected = self.expected_identity(agent);
                let session = client_handshake(&mut stream, signing, expected.as_ref()).await?;
                ReplyPath::Secure(attach_secure(stream, session, self.inbound.clone()))
            }
        };
        self.pool.insert(*agent, transport.clone());
        Ok(transport)
    }

    /// The mesh identity expected at a peer's address, if it has been recorded.
    fn expected_identity(&self, agent: &AgentId) -> Option<[u8; 32]> {
        self.identities.get(agent).map(|e| *e.value())
    }

    /// Send to one peer, re-dialing once if the pooled connection is dead.
    async fn deliver(&self, target: AgentId, envelope: &MeshEnvelope) -> Result<(), NameMeshError> {
        let conn = self.connection(&target).await?;
        if conn.send(envelope.clone()).await.is_ok() {
            return Ok(());
        }
        // The pooled socket was stale — most often a peer that restarted. Retry
        // once on a fresh connection, then report honestly.
        self.pool.remove(&target);
        let conn = self.connection(&target).await?;
        conn.send(envelope.clone()).await
    }

    /// Open a connection to a bare address, for a peer with no known identity.
    ///
    /// Deliberately not pooled by [`AgentId`] — there is no agent id yet, which
    /// is the entire reason this path exists. It is keyed by endpoint instead and
    /// held only until bootstrap learns who answered, because the connection has
    /// to outlive the send: the ack comes back on it.
    ///
    /// The handshake here is unpinned even in encrypted mode. There is nothing to
    /// pin it to — the identity is what we are asking for — so the channel is
    /// confidential but the peer is only authenticated afterwards, by the
    /// signature over the key its ack carries.
    async fn provisional(&self, endpoint: &str) -> Result<ReplyPath, NameMeshError> {
        if let Some(existing) = self.bootstrap.get(endpoint) {
            return Ok(existing.value().clone());
        }

        let addr: SocketAddr = endpoint
            .parse()
            .map_err(|e| NameMeshError::Transport(format!("bad seed address `{endpoint}`: {e}")))?;

        let mut stream = TcpStream::connect(addr)
            .await
            .map_err(|e| NameMeshError::Transport(format!("dial seed {addr}: {e}")))?;
        let _ = stream.set_nodelay(true);

        let path = match &self.security {
            Security::Plaintext => ReplyPath::Plain(attach(stream, self.inbound.clone())),
            Security::Encrypted(cfg) => {
                // Nothing to pin to: establishing the identity is the point of
                // dialing a bootstrap address.
                let session = client_handshake(&mut stream, &cfg.signing, None).await?;
                ReplyPath::Secure(attach_secure(stream, session, self.inbound.clone()))
            }
        };
        self.bootstrap.insert(endpoint.to_string(), path.clone());
        Ok(path)
    }
}

#[async_trait]
impl NameTransport for TcpNameTransport {
    async fn send(&self, envelope: MeshEnvelope) -> Result<(), NameMeshError> {
        match envelope.to {
            crate::mesh::MeshTarget::Agent(id) => self.deliver(id, &envelope).await,
            // A broadcast goes to every peer we can reach. Individual failures
            // are not fatal: an announcement is best-effort by nature.
            _ => {
                let peers: Vec<AgentId> = self.addresses.iter().map(|e| *e.key()).collect();
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

    /// Take the first endpoint that parses as a socket address.
    ///
    /// A peer may advertise `ws://…` or a hostname this transport cannot use;
    /// those are skipped rather than treated as an error, because the peer is
    /// describing itself to every transport at once, not just this one.
    async fn learn(&self, agent: AgentId, endpoints: &[String]) {
        if let Some(addr) = endpoints.iter().find_map(|e| e.parse::<SocketAddr>().ok()) {
            self.set_address(agent, addr);
        }
    }

    async fn release(&self, endpoint: &str) {
        self.bootstrap.remove(endpoint);
    }
}

// ───────────────────────────────── Listener ─────────────────────────────────

/// Accepts inbound mesh connections and feeds them to the same inbound stream.
#[derive(Debug)]
pub struct MeshListener {
    listener: TcpListener,
}

impl MeshListener {
    /// Bind a listener. Pass port 0 to let the OS choose.
    pub async fn bind(addr: SocketAddr) -> Result<Self, NameMeshError> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| NameMeshError::Transport(format!("bind {addr}: {e}")))?;
        Ok(Self { listener })
    }

    /// The address actually bound — the real port when 0 was requested.
    pub fn local_addr(&self) -> Result<SocketAddr, NameMeshError> {
        self.listener
            .local_addr()
            .map_err(|e| NameMeshError::Transport(format!("local_addr: {e}")))
    }

    /// Accept until the task is dropped, attaching each connection.
    ///
    /// Each connection reads on its own task, so one slow or malformed peer
    /// cannot block the others.
    pub async fn serve(self, inbound: mpsc::Sender<Inbound>) {
        self.serve_with(inbound, None).await
    }

    /// Accept connections, performing a server handshake when `signing` is given.
    ///
    /// The handshake runs on the connection's own task, so a peer that stalls
    /// mid-handshake cannot hold up the accept loop.
    pub async fn serve_with(self, inbound: mpsc::Sender<Inbound>, signing: Option<SigningKey>) {
        loop {
            match self.listener.accept().await {
                Ok((mut stream, peer)) => {
                    let _ = stream.set_nodelay(true);
                    let inbound = inbound.clone();
                    match signing.clone() {
                        None => {
                            attach(stream, inbound);
                        }
                        Some(key) => {
                            tokio::spawn(async move {
                                match server_handshake(&mut stream, &key).await {
                                    Ok(session) => {
                                        attach_secure(stream, session, inbound);
                                    }
                                    Err(e) => {
                                        tracing::debug!("handshake with {peer} failed: {e}")
                                    }
                                }
                            });
                        }
                    }
                }
                Err(e) => tracing::warn!("mesh accept failed: {e}"),
            }
        }
    }
}

/// Drive a resolver from an inbound stream until it closes.
///
/// Each envelope is answered on the connection it arrived on.
pub async fn pump(mut inbound: mpsc::Receiver<Inbound>, resolver: Arc<MeshNameResolver>) {
    while let Some(item) = inbound.recv().await {
        let now = resolver.now();
        let reply = item.reply.clone();
        resolver
            .handle_envelope_with_reply(&item.envelope, now, Some(&reply))
            .await;
    }
}

/// Bind a listener, wire it to `resolver`, and start serving.
///
/// Returns the bound address. This is the one call a node needs to join the
/// mesh; everything above it is available separately for custom wiring.
pub async fn serve_node(
    bind_addr: SocketAddr,
    resolver: Arc<MeshNameResolver>,
    transport: Arc<TcpNameTransport>,
    inbound: mpsc::Receiver<Inbound>,
) -> Result<SocketAddr, NameMeshError> {
    serve_node_with(bind_addr, resolver, transport, inbound, None).await
}

/// As [`serve_node`], with an explicit signing key for accepted connections.
///
/// Pass the node's mesh identity when the transport is encrypted, so both
/// directions use the same key — the one that also signs its records.
pub async fn serve_node_with(
    bind_addr: SocketAddr,
    resolver: Arc<MeshNameResolver>,
    transport: Arc<TcpNameTransport>,
    inbound: mpsc::Receiver<Inbound>,
    signing: Option<SigningKey>,
) -> Result<SocketAddr, NameMeshError> {
    let listener = MeshListener::bind(bind_addr).await?;
    let addr = listener.local_addr()?;
    tokio::spawn(listener.serve_with(transport.inbound_sender(), signing));
    tokio::spawn(pump(inbound, resolver));
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::SigningIdentity;
    use crate::mesh::{MeshConfig, MeshNode, MeshPayload};
    use crate::naming::ResolveQuery;
    use ed25519_dalek::SigningKey;
    use spine_name::{Endpoint, NameRecord, SpineUri};
    use std::time::Duration;

    const NOW: u64 = 1_700_000_000;

    fn record(seed: u8, caps: &[&str]) -> NameRecord {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name, 1, NOW)
            .unwrap()
            .with_ttl(3600)
            .with_endpoint(Endpoint::new("tcp", format!("10.0.0.{seed}:9440")));
        for c in caps {
            rec = rec.with_capability(*c);
        }
        rec.sign(&key).unwrap();
        rec
    }

    /// One fully networked node: identity, resolver, transport, live listener.
    struct Node {
        resolver: Arc<MeshNameResolver>,
        transport: Arc<TcpNameTransport>,
        mesh: Arc<MeshNode>,
        addr: SocketAddr,
    }

    async fn spawn_node(seed: u8) -> Node {
        let identity = SigningIdentity::from_seed(&format!("tcp{seed}"), [200 + seed; 32]);
        let mesh = Arc::new(MeshNode::new(identity, MeshConfig::default()));
        let (transport, inbound) = TcpNameTransport::new();
        let resolver = Arc::new(
            MeshNameResolver::new(mesh.clone(), transport.clone())
                .with_clock(|| NOW)
                .with_timeout(Duration::from_secs(5)),
        );
        let addr = serve_node(
            "127.0.0.1:0".parse().unwrap(),
            resolver.clone(),
            transport.clone(),
            inbound,
        )
        .await
        .unwrap();

        Node {
            resolver,
            transport,
            mesh,
            addr,
        }
    }

    /// Introduce b to a, in the address book and in both routing spaces.
    async fn introduce(a: &Node, b: &Node) {
        a.transport.set_address(*b.mesh.agent_id(), b.addr);
        a.resolver
            .register_peer(&b.mesh.public_identity(), vec![b.addr.to_string()], NOW)
            .await;
    }

    // ── Framing ──

    #[tokio::test]
    async fn framing_roundtrips_an_envelope() {
        let node = MeshNode::new(
            SigningIdentity::from_seed("framing", [1u8; 32]),
            MeshConfig::default(),
        );
        let envelope = node.announce_name(record(1, &["web.search"])).unwrap();

        let mut buf = Vec::new();
        write_envelope(&mut buf, &envelope).await.unwrap();
        assert_eq!(
            u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize,
            buf.len() - 4,
            "the length prefix must describe the body exactly"
        );

        let mut cursor = std::io::Cursor::new(buf);
        let back = read_envelope(&mut cursor).await.unwrap().unwrap();
        assert_eq!(back.id, envelope.id);
        match &back.payload {
            MeshPayload::NameAnnounce(a) => {
                assert!(
                    a.record.verify().is_ok(),
                    "a signature must survive the wire"
                )
            }
            other => panic!("expected NameAnnounce, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn several_envelopes_stream_back_to_back() {
        let node = MeshNode::new(
            SigningIdentity::from_seed("stream", [2u8; 32]),
            MeshConfig::default(),
        );
        let mut buf = Vec::new();
        for i in 0..5u64 {
            let e = node.name_resolve_request(
                *node.agent_id(),
                i,
                ResolveQuery::Capability("web.search".into()),
            );
            write_envelope(&mut buf, &e).await.unwrap();
        }

        let mut cursor = std::io::Cursor::new(buf);
        for i in 0..5u64 {
            match read_envelope(&mut cursor).await.unwrap().unwrap().payload {
                MeshPayload::NameResolveRequest(r) => assert_eq!(r.request_id, i),
                other => panic!("expected a request, got {other:?}"),
            }
        }
        assert!(read_envelope(&mut cursor).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_clean_end_of_stream_is_not_an_error() {
        let mut empty = std::io::Cursor::new(Vec::new());
        assert!(read_envelope(&mut empty).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn an_oversized_frame_is_refused_before_allocating() {
        let mut header = u32::MAX.to_be_bytes().to_vec();
        header.extend_from_slice(b"not actually this long");
        let mut cursor = std::io::Cursor::new(header);
        let err = read_envelope(&mut cursor).await.unwrap_err();
        assert!(
            err.to_string().contains("over the"),
            "expected a size refusal, got: {err}"
        );
    }

    #[tokio::test]
    async fn truncated_and_corrupt_frames_are_reported_not_accepted() {
        let mut truncated = 100u32.to_be_bytes().to_vec();
        truncated.extend_from_slice(b"abc");
        assert!(read_envelope(&mut std::io::Cursor::new(truncated))
            .await
            .is_err());

        let body = b"{not json";
        let mut corrupt = (body.len() as u32).to_be_bytes().to_vec();
        corrupt.extend_from_slice(body);
        assert!(read_envelope(&mut std::io::Cursor::new(corrupt))
            .await
            .is_err());

        assert!(read_envelope(&mut std::io::Cursor::new(vec![0, 0, 0, 0]))
            .await
            .is_err());
    }

    // ── Addressing ──

    #[tokio::test]
    async fn sending_to_an_unknown_peer_fails_with_a_clear_reason() {
        let (transport, _rx) = TcpNameTransport::new();
        let node = MeshNode::new(
            SigningIdentity::from_seed("lonely", [3u8; 32]),
            MeshConfig::default(),
        );
        let envelope = node.name_resolve_request(
            *node.agent_id(),
            1,
            ResolveQuery::Name(SpineUri::did([1u8; 32])),
        );
        let err = transport.send(envelope).await.unwrap_err();
        assert!(err.to_string().contains("no address known"), "{err}");
    }

    // ── Over real sockets ──

    #[tokio::test]
    async fn a_name_resolves_over_a_real_tcp_connection() {
        let seeker = spawn_node(1).await;
        let holder = spawn_node(2).await;
        introduce(&seeker, &holder).await;

        let rec = record(1, &[]);
        holder.resolver.publish_local(rec.clone()).await.unwrap();

        let found = seeker.resolver.resolve(&rec.name).await.unwrap();
        assert_eq!(found, rec, "resolved across a socket, not a test harness");
        assert!(found.verify().is_ok());
        assert_eq!(seeker.transport.pooled_connections(), 1);
    }

    // ── Bootstrap ──

    /// The entry-point problem, over a real socket: a node given nothing but an
    /// address ends up with a peer it can dial and an identity it verified.
    #[tokio::test]
    async fn a_node_bootstraps_into_the_mesh_from_an_address_alone() {
        let seed = spawn_node(40).await;
        let newcomer = spawn_node(41).await;
        seed.resolver
            .set_endpoints(vec![seed.addr.to_string()])
            .await;

        assert_eq!(newcomer.resolver.peer_count().await, 0);
        assert_eq!(newcomer.transport.known_addresses(), 0);

        let report = newcomer.resolver.bootstrap(&[seed.addr.to_string()]).await;

        assert!(report.is_connected(), "bootstrap failed: {report:?}");
        assert_eq!(report.reached[0].key, seed.resolver.local_key());
        assert_eq!(newcomer.resolver.peer_count().await, 1);
        assert_eq!(
            newcomer.transport.known_addresses(),
            1,
            "knowing a peer must mean being able to dial it"
        );

        // The proof it is a real contact and not just a table entry.
        let rec = record(1, &[]);
        seed.resolver.publish_local(rec.clone()).await.unwrap();
        assert_eq!(newcomer.resolver.resolve(&rec.name).await.unwrap(), rec);
    }

    /// The payoff, and the thing that did not work before this: a lookup reaches
    /// a node it was never introduced to, by following a referral to it.
    ///
    /// Referrals used to carry only a keyspace position, so the seeker could
    /// place the holder in its shortlist and had no way to address it — every
    /// walk stopped at the peers someone had configured by hand.
    #[tokio::test]
    async fn a_lookup_reaches_a_peer_it_was_never_introduced_to() {
        let seeker = spawn_node(42).await;
        let middle = spawn_node(43).await;
        let holder = spawn_node(44).await;

        // The seeker knows only the middle node; the middle node knows the
        // holder. Nobody tells the seeker how to reach the holder.
        introduce(&seeker, &middle).await;
        middle
            .resolver
            .register_peer(
                &holder.mesh.public_identity(),
                vec![holder.addr.to_string()],
                NOW,
            )
            .await;

        let rec = record(7, &[]);
        holder.resolver.publish_local(rec.clone()).await.unwrap();

        assert_eq!(
            seeker.transport.known_addresses(),
            1,
            "the seeker starts out able to dial exactly one peer"
        );

        let found = seeker.resolver.resolve(&rec.name).await.unwrap();
        assert_eq!(found, rec, "the walk must reach a peer it was referred to");
        assert_eq!(
            seeker.transport.known_addresses(),
            2,
            "the referral taught the transport a new address"
        );
    }

    /// What replication is for, over real sockets: a name is answerable by a
    /// node that never published it, so the publisher is no longer the single
    /// point of failure for its own names.
    #[tokio::test]
    async fn a_published_record_is_served_by_a_node_that_did_not_publish_it() {
        let publisher = spawn_node(45).await;
        let holder = spawn_node(46).await;
        let seeker = spawn_node(47).await;

        // The seeker and the publisher share only the holder. Nothing connects
        // the seeker to the node the record came from.
        introduce(&publisher, &holder).await;
        introduce(&seeker, &holder).await;

        let rec = record(9, &[]);
        let report = publisher.resolver.publish(rec.clone()).await.unwrap();
        assert!(report.is_durable(), "no copy was placed: {report:?}");

        // An announcement is fire-and-forget: `publish` returns once the copy
        // is on the wire, and there is no ack to await. Wait for the holder to
        // have processed it rather than assuming a send is an arrival.
        let mut held = 0;
        for _ in 0..50 {
            held = holder.resolver.record_count().await;
            if held == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(held, 1, "the holder never stored the copy");

        assert_eq!(
            seeker.resolver.resolve(&rec.name).await.unwrap(),
            rec,
            "the copy answered on the publisher's behalf"
        );
    }

    #[tokio::test]
    async fn the_responder_answers_without_knowing_how_to_dial_the_asker() {
        // The property that makes this work through NAT: only the seeker knows
        // where the holder is, never the reverse.
        let seeker = spawn_node(21).await;
        let holder = spawn_node(22).await;
        introduce(&seeker, &holder).await;
        assert_eq!(
            holder.transport.known_addresses(),
            0,
            "the holder must have no way to dial the seeker"
        );

        let rec = record(1, &[]);
        holder.resolver.publish_local(rec.clone()).await.unwrap();
        assert_eq!(seeker.resolver.resolve(&rec.name).await.unwrap(), rec);
    }

    #[tokio::test]
    async fn a_capability_resolves_over_tcp_from_two_holders() {
        let seeker = spawn_node(3).await;
        let a = spawn_node(4).await;
        let b = spawn_node(5).await;
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
        assert_eq!(
            providers.len(),
            2,
            "both holders answered; neither was dropped"
        );
        assert!(providers.iter().all(|p| p.verify().is_ok()));
    }

    #[tokio::test]
    async fn an_announcement_broadcasts_to_every_known_peer() {
        let publisher = spawn_node(6).await;
        let a = spawn_node(7).await;
        let b = spawn_node(8).await;
        introduce(&publisher, &a).await;
        introduce(&publisher, &b).await;

        publisher
            .resolver
            .publish(record(1, &["web.search"]))
            .await
            .unwrap();

        for _ in 0..100 {
            if a.resolver.record_count().await > 0 && b.resolver.record_count().await > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(a.resolver.record_count().await, 1);
        assert_eq!(b.resolver.record_count().await, 1);
    }

    #[tokio::test]
    async fn a_forged_record_is_rejected_after_crossing_the_wire() {
        let publisher = spawn_node(23).await;
        let receiver = spawn_node(24).await;
        introduce(&publisher, &receiver).await;

        let mut forged = record(1, &[]);
        forged.capabilities.push("admin".into()); // breaks the signature
        let envelope = publisher.mesh.create_envelope(
            crate::mesh::MeshTarget::Broadcast,
            MeshPayload::NameAnnounce(Box::new(crate::naming::AnnouncedRecord::new(forged))),
        );
        publisher.transport.send(envelope).await.unwrap();

        for _ in 0..50 {
            if receiver.resolver.metrics().await.announcements_rejected > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(receiver.resolver.record_count().await, 0);
        assert_eq!(receiver.resolver.metrics().await.announcements_rejected, 1);
    }

    #[tokio::test]
    async fn a_connection_is_reused_across_lookups() {
        let seeker = spawn_node(9).await;
        let holder = spawn_node(10).await;
        introduce(&seeker, &holder).await;

        holder.resolver.publish_local(record(1, &[])).await.unwrap();
        holder.resolver.publish_local(record(2, &[])).await.unwrap();

        seeker.resolver.resolve(&record(1, &[]).name).await.unwrap();
        seeker.resolver.resolve(&record(2, &[]).name).await.unwrap();

        assert_eq!(
            seeker.transport.pooled_connections(),
            1,
            "a second lookup must not re-handshake"
        );
    }

    #[tokio::test]
    async fn an_evicted_connection_is_re_dialed_transparently() {
        let seeker = spawn_node(11).await;
        let holder = spawn_node(12).await;
        introduce(&seeker, &holder).await;
        holder.resolver.publish_local(record(1, &[])).await.unwrap();

        seeker.resolver.resolve(&record(1, &[]).name).await.unwrap();
        seeker.transport.evict(holder.mesh.agent_id());
        assert_eq!(seeker.transport.pooled_connections(), 0);

        let again = seeker.resolver.resolve(&record(1, &[]).name).await.unwrap();
        assert_eq!(
            again,
            record(1, &[]),
            "re-dialed without the caller noticing"
        );
        assert_eq!(seeker.transport.pooled_connections(), 1);
    }

    #[tokio::test]
    async fn re_announcing_the_same_address_keeps_the_connection() {
        let seeker = spawn_node(13).await;
        let holder = spawn_node(14).await;
        introduce(&seeker, &holder).await;
        holder.resolver.publish_local(record(1, &[])).await.unwrap();
        seeker.resolver.resolve(&record(1, &[]).name).await.unwrap();
        assert_eq!(seeker.transport.pooled_connections(), 1);

        // Gossip repeats an address constantly; dropping a healthy connection
        // each time would be a self-inflicted reconnect storm.
        seeker
            .transport
            .set_address(*holder.mesh.agent_id(), holder.addr);
        assert_eq!(seeker.transport.pooled_connections(), 1);

        // A genuinely different address does invalidate it.
        seeker
            .transport
            .set_address(*holder.mesh.agent_id(), "127.0.0.1:1".parse().unwrap());
        assert_eq!(seeker.transport.pooled_connections(), 0);
    }

    #[tokio::test]
    async fn an_unresolvable_name_reports_not_found_over_tcp() {
        let seeker = spawn_node(15).await;
        let holder = spawn_node(16).await;
        introduce(&seeker, &holder).await;

        let err = seeker
            .resolver
            .resolve(&record(99, &[]).name)
            .await
            .unwrap_err();
        assert!(matches!(err, NameMeshError::NotFound(_)), "{err}");
    }

    #[tokio::test]
    async fn a_peer_that_is_down_does_not_hang_the_lookup() {
        let seeker = spawn_node(17).await;
        let ghost = spawn_node(18).await;
        introduce(&seeker, &ghost).await;
        // Point the address book at a port nothing is listening on.
        seeker
            .transport
            .set_address(*ghost.mesh.agent_id(), "127.0.0.1:1".parse().unwrap());

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            seeker.resolver.resolve(&record(1, &[]).name),
        )
        .await
        .expect("resolve() hung on an unreachable peer");

        assert!(result.is_err());
        assert!(seeker.resolver.metrics().await.unroutable_peers >= 1);
    }

    #[tokio::test]
    async fn a_malformed_frame_closes_only_its_own_connection() {
        let holder = spawn_node(19).await;
        holder.resolver.publish_local(record(1, &[])).await.unwrap();

        // A hostile client announces an absurd frame and gets dropped.
        let mut bad = TcpStream::connect(holder.addr).await.unwrap();
        bad.write_all(&[0xFF, 0xFF, 0xFF, 0xFF]).await.unwrap();
        let _ = bad.flush().await;
        drop(bad);

        // A well-behaved peer is unaffected.
        let seeker = spawn_node(20).await;
        introduce(&seeker, &holder).await;
        assert_eq!(
            seeker.resolver.resolve(&record(1, &[]).name).await.unwrap(),
            record(1, &[]),
            "one bad peer must not take down the node"
        );
    }
}

#[cfg(test)]
mod secure_tests {
    use super::*;
    use crate::identity::SigningIdentity;
    use crate::mesh::{MeshConfig, MeshNode};
    use ed25519_dalek::SigningKey;
    use spine_name::{Endpoint, NameRecord, SpineUri};
    use std::time::Duration;

    const NOW: u64 = 1_700_000_000;

    fn record(seed: u8, caps: &[&str]) -> NameRecord {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name, 1, NOW)
            .unwrap()
            .with_ttl(3600)
            .with_endpoint(Endpoint::new("tcp", format!("10.0.0.{seed}:9440")));
        for c in caps {
            rec = rec.with_capability(*c);
        }
        rec.sign(&key).unwrap();
        rec
    }

    struct SecureNode {
        resolver: Arc<MeshNameResolver>,
        transport: Arc<TcpNameTransport>,
        mesh: Arc<MeshNode>,
        addr: SocketAddr,
        signing: SigningKey,
    }

    /// A node whose mesh transport is encrypted with its own mesh identity.
    async fn spawn_secure(seed: u8) -> SecureNode {
        let identity = SigningIdentity::from_seed(&format!("sec{seed}"), [50 + seed; 32]);
        // The handshake key and the mesh identity are the same Ed25519 key.
        let signing = SigningKey::from_bytes(&[50 + seed; 32]);
        let mesh = Arc::new(MeshNode::new(identity, MeshConfig::default()));
        let (transport, inbound) = TcpNameTransport::encrypted(signing.clone());
        let resolver = Arc::new(
            MeshNameResolver::new(mesh.clone(), transport.clone())
                .with_clock(|| NOW)
                .with_timeout(Duration::from_secs(5)),
        );
        let addr = serve_node_with(
            "127.0.0.1:0".parse().unwrap(),
            resolver.clone(),
            transport.clone(),
            inbound,
            Some(signing.clone()),
        )
        .await
        .unwrap();

        SecureNode {
            resolver,
            transport,
            mesh,
            addr,
            signing,
        }
    }

    async fn introduce(a: &SecureNode, b: &SecureNode) {
        a.transport.set_address(*b.mesh.agent_id(), b.addr);
        a.transport
            .set_identity(*b.mesh.agent_id(), b.signing.verifying_key().to_bytes());
        a.resolver
            .register_peer(&b.mesh.public_identity(), vec![b.addr.to_string()], NOW)
            .await;
    }

    #[tokio::test]
    async fn a_name_resolves_over_an_encrypted_connection() {
        let seeker = spawn_secure(1).await;
        let holder = spawn_secure(2).await;
        introduce(&seeker, &holder).await;
        assert!(seeker.transport.is_encrypted());

        let rec = record(1, &[]);
        holder.resolver.publish_local(rec.clone()).await.unwrap();

        let found = seeker.resolver.resolve(&rec.name).await.unwrap();
        assert_eq!(
            found, rec,
            "resolved over an authenticated, encrypted socket"
        );
        assert!(found.verify().is_ok());
    }

    #[tokio::test]
    async fn a_capability_resolves_over_encrypted_connections() {
        let seeker = spawn_secure(3).await;
        let a = spawn_secure(4).await;
        let b = spawn_secure(5).await;
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
    async fn the_query_is_not_readable_on_the_wire() {
        // The property that matters: an eavesdropper on the socket must not be
        // able to see which name an agent is resolving.
        let holder = spawn_secure(6).await;
        let rec = record(1, &["web.search"]);
        holder.resolver.publish_local(rec.clone()).await.unwrap();

        // Dial the holder directly and complete a handshake as an ordinary peer.
        let mut stream = TcpStream::connect(holder.addr).await.unwrap();
        let observer_key = SigningKey::from_bytes(&[123u8; 32]);
        let mut session = client_handshake(
            &mut stream,
            &observer_key,
            Some(&holder.signing.verifying_key().to_bytes()),
        )
        .await
        .unwrap();

        let node = MeshNode::new(
            SigningIdentity::from_seed("observer", [123u8; 32]),
            MeshConfig::default(),
        );
        let request = node.name_resolve_request(
            *holder.mesh.agent_id(),
            1,
            crate::naming::ResolveQuery::Name(rec.name.clone()),
        );
        let plaintext = serde_json::to_vec(&request).unwrap();
        let sealed = session.seal(&plaintext);

        // The name is plainly present before sealing and absent after.
        let needle = rec.name.to_string();
        assert!(plaintext
            .windows(needle.len())
            .any(|w| w == needle.as_bytes()));
        assert!(
            !sealed.windows(needle.len()).any(|w| w == needle.as_bytes()),
            "the resolved name must not appear in ciphertext"
        );

        // And the sealed frame is genuinely accepted by the peer.
        write_frame(&mut stream, &sealed).await.unwrap();
        let reply = read_frame(&mut stream, MAX_FRAME_BYTES + 1024)
            .await
            .unwrap()
            .unwrap();
        let opened = session.open(&reply).unwrap();
        let response: MeshEnvelope = serde_json::from_slice(&opened).unwrap();
        match response.payload {
            crate::mesh::MeshPayload::NameResolveResponse(r) => {
                assert_eq!(r.record.as_ref(), Some(&rec))
            }
            other => panic!("expected a response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_dialer_refuses_a_peer_with_the_wrong_identity() {
        let holder = spawn_secure(8).await;
        let mut stream = TcpStream::connect(holder.addr).await.unwrap();

        // Expect somebody else's key at that address.
        let wrong = SigningKey::from_bytes(&[200u8; 32])
            .verifying_key()
            .to_bytes();
        let err = client_handshake(
            &mut stream,
            &SigningKey::from_bytes(&[9u8; 32]),
            Some(&wrong),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("not the expected"),
            "a mismatched identity must abort the connection: {err}"
        );
    }

    #[tokio::test]
    async fn a_plaintext_client_cannot_talk_to_an_encrypted_node() {
        let holder = spawn_secure(9).await;
        holder.resolver.publish_local(record(1, &[])).await.unwrap();

        // Send an unencrypted envelope: the server reads it as a handshake,
        // fails to parse it, and drops the connection.
        let mut stream = TcpStream::connect(holder.addr).await.unwrap();
        let node = MeshNode::new(
            SigningIdentity::from_seed("plain", [77u8; 32]),
            MeshConfig::default(),
        );
        let envelope = node.name_resolve_request(
            *holder.mesh.agent_id(),
            1,
            crate::naming::ResolveQuery::Name(record(1, &[]).name),
        );
        let _ = write_envelope(&mut stream, &envelope).await;

        // No reply ever arrives.
        let got = tokio::time::timeout(
            Duration::from_millis(500),
            read_frame(&mut stream, MAX_FRAME_BYTES),
        )
        .await;
        let unanswered = match got {
            Err(_) => true,       // timed out
            Ok(Ok(None)) => true, // clean close
            Ok(Err(_)) => true,   // reset
            Ok(Ok(Some(_))) => false,
        };
        assert!(unanswered, "an unauthenticated peer must not be served");
        assert_eq!(holder.resolver.metrics().await.requests_answered, 0);
    }

    #[tokio::test]
    async fn a_tampered_frame_drops_the_connection() {
        let holder = spawn_secure(10).await;
        holder.resolver.publish_local(record(1, &[])).await.unwrap();

        let mut stream = TcpStream::connect(holder.addr).await.unwrap();
        let key = SigningKey::from_bytes(&[124u8; 32]);
        let mut session = client_handshake(
            &mut stream,
            &key,
            Some(&holder.signing.verifying_key().to_bytes()),
        )
        .await
        .unwrap();

        let node = MeshNode::new(
            SigningIdentity::from_seed("tamper", [124u8; 32]),
            MeshConfig::default(),
        );
        let request = node.name_resolve_request(
            *holder.mesh.agent_id(),
            1,
            crate::naming::ResolveQuery::Name(record(1, &[]).name),
        );
        let mut sealed = session.seal(&serde_json::to_vec(&request).unwrap());
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01; // flip a bit in the tag

        write_frame(&mut stream, &sealed).await.unwrap();

        let got = tokio::time::timeout(
            Duration::from_millis(500),
            read_frame(&mut stream, MAX_FRAME_BYTES),
        )
        .await;
        let refused = !matches!(got, Ok(Ok(Some(_))));
        assert!(refused, "a forged frame must not be answered");
        assert_eq!(holder.resolver.metrics().await.requests_answered, 0);
    }

    #[tokio::test]
    async fn encrypted_connections_are_pooled_and_reused() {
        let seeker = spawn_secure(11).await;
        let holder = spawn_secure(12).await;
        introduce(&seeker, &holder).await;

        holder.resolver.publish_local(record(1, &[])).await.unwrap();
        holder.resolver.publish_local(record(2, &[])).await.unwrap();

        seeker.resolver.resolve(&record(1, &[]).name).await.unwrap();
        seeker.resolver.resolve(&record(2, &[]).name).await.unwrap();

        assert_eq!(
            seeker.transport.pooled_connections(),
            1,
            "a second lookup must not repeat the KEM handshake"
        );
    }

    #[tokio::test]
    async fn an_announcement_propagates_over_encrypted_links() {
        let publisher = spawn_secure(13).await;
        let receiver = spawn_secure(14).await;
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
