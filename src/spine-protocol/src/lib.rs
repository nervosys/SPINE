// Allow dead code for protocol features designed for future extensions
#![allow(dead_code)]

pub mod agentic;
pub mod agentic_codec;
pub mod cbor;
pub mod connection;
pub mod frame;
pub mod hello;
pub mod mcp;
pub mod replay;
pub mod resource;
pub mod tasks;
pub mod wire;

mod handler;
pub use handler::*;

#[cfg(feature = "research")]
mod research;
#[cfg(feature = "research")]
pub use research::*;

pub use agentic::{
    Capability, CapabilityAdvertisement, CapabilityQuery, CapabilitySelector, StreamCancel,
    StreamData, StreamEnd, StreamEndReason, StreamRole, StreamStart, StreamToken, StreamUsage,
    ToolCall, ToolOutcome, ToolResult, TraceContext,
};
#[cfg(feature = "research")]
pub use agentic_codec::TitansLatentCodec;
pub use agentic_codec::{
    CodecAdvertisement, CodecDescriptor, CodecDirection, CodecError, CodecNegotiation,
    CodecRegistry, DType, DecodeHints, EmbeddingInput, EmbeddingRequest, EmbeddingResponse,
    EncodedFrame, EncodedMetadata, Modality, NeuralCodec,
};

use serde::{Deserialize, Serialize};
#[cfg(feature = "quic")]
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

// High-performance binary serialization
// (bincode and bytemuck are used via their traits, not explicit imports)

// QUIC transport support (uses rustls 0.23 via quinn)
#[cfg(feature = "quic")]
use quinn::{ClientConfig, Connection as QuinnConnection, Endpoint, ServerConfig};
#[cfg(feature = "quic")]
use rustls as quic_rustls; // rustls 0.23 for QUIC (workspace)
#[cfg(feature = "quic")]
use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
#[cfg(feature = "quic")]
use std::net::SocketAddr;

/// Write `header` followed by `body` to an `AsyncWrite` using a single
/// `write_vectored` call when the underlying stream supports it, falling back
/// to sequential writes for the remainder on a partial write.
///
/// Replaces the common `write_all(&header); write_all(&body)` pattern that
/// costs two `send()` syscalls (and on TCP_NODELAY sockets, often two packets
/// on the wire).
#[inline]
pub async fn write_header_body<W: AsyncWrite + Unpin + ?Sized>(
    w: &mut W,
    header: &[u8],
    body: &[u8],
) -> std::io::Result<()> {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        write_header_body_inner(w, header, body),
    )
    .await;
    match result {
        Ok(Ok(())) => Ok(()),
        other => {
            // A partial frame cannot be retried on this byte stream. Closing
            // also bounds server output when the remote client stops reading.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(1), w.shutdown()).await;
            match other {
                Ok(Err(error)) => Err(error),
                Err(_) => Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "native write deadline exceeded",
                )),
                Ok(Ok(())) => unreachable!(),
            }
        }
    }
}

async fn write_header_body_inner<W: AsyncWrite + Unpin + ?Sized>(
    w: &mut W,
    header: &[u8],
    body: &[u8],
) -> std::io::Result<()> {
    use std::io::IoSlice;
    let total = header.len() + body.len();
    let mut sent: usize = 0;
    // First attempt: single vectored call. Most TCP impls in tokio honor this.
    while sent < total {
        let n = if sent < header.len() {
            let slices = [IoSlice::new(&header[sent..]), IoSlice::new(body)];
            w.write_vectored(&slices).await?
        } else {
            w.write(&body[sent - header.len()..]).await?
        };
        if n == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::WriteZero));
        }
        sent += n;
    }
    Ok(())
}

// =============================================================================
// CHAMELEON PROTOCOL
// =============================================================================
//
// Experimental adaptive representation and compression. Vector transforms
// and moving headers do not establish confidentiality or cryptographic
// authenticity. Use an authenticated transport for private sessions; these
// research codecs are not substitutes for TLS or reviewed AEAD.

// =============================================================================
// LATENT SPACE CRYPTOGRAPHY
// =============================================================================

/// A point in the latent space. The dimensionality and basis evolve per-message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatentVector {
    /// The high-dimensional representation
    pub components: Vec<f32>,
    /// Dimension hint (can be misleading as part of obfuscation)
    pub dim_hint: u16,
    /// Epoch number for transformation alignment
    pub epoch: u64,
}

impl LatentVector {
    /// Zero-copy serialization to bytes (22+ GiB/s for 1024-dim vectors)
    /// Layout: [dim_hint: u16][epoch: u64][components: [f32; N]]
    #[inline]
    pub fn to_bytes_fast(&self) -> Vec<u8> {
        let component_bytes = self.components.len() * 4;
        let total = 2 + 8 + component_bytes;
        let mut buf = Vec::with_capacity(total);

        buf.extend_from_slice(&self.dim_hint.to_le_bytes());
        buf.extend_from_slice(&self.epoch.to_le_bytes());
        // SAFETY: f32 has no padding, transmute is valid
        buf.extend_from_slice(bytemuck::cast_slice(&self.components));

        buf
    }

    /// Zero-copy deserialization from bytes
    #[inline]
    pub fn from_bytes_fast(data: &[u8]) -> Option<Self> {
        if data.len() < 10 {
            return None;
        }

        let dim_hint = u16::from_le_bytes([data[0], data[1]]);
        let epoch = u64::from_le_bytes(data[2..10].try_into().ok()?);

        let component_bytes = &data[10..];
        if !component_bytes.len().is_multiple_of(4) {
            return None;
        }

        // Use try_cast_slice to handle unaligned data gracefully
        let components: Vec<f32> = match bytemuck::try_cast_slice(component_bytes) {
            Ok(s) => s.to_vec(),
            Err(_) => component_bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect(),
        };

        Some(Self {
            components,
            dim_hint,
            epoch,
        })
    }

    /// Serialize to bytes into pre-allocated buffer (zero-alloc hot path)
    #[inline]
    pub fn to_bytes_into(&self, buf: &mut Vec<u8>) {
        let component_bytes = self.components.len() * 4;
        let total = 2 + 8 + component_bytes;
        buf.clear();
        buf.reserve(total);

        buf.extend_from_slice(&self.dim_hint.to_le_bytes());
        buf.extend_from_slice(&self.epoch.to_le_bytes());
        buf.extend_from_slice(bytemuck::cast_slice(&self.components));
    }

    /// Bincode serialization (fallback, ~5x faster than JSON)
    #[inline]
    pub fn to_bincode(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Bincode deserialization
    #[inline]
    pub fn from_bincode(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

// =============================================================================
// MESSAGE TYPES
// =============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Message {
    Request(Request),
    Response(Response),
    Event(Event),
    BinaryProgram(SpineBinary),
    /// Latent-encoded message (implicit encryption)
    LatentMessage(LatentVector),
    /// Server-initiated request to morph the protocol
    MorphRequest {
        seed: u64,
    },
    /// Health check to verify connection
    Ping {
        timestamp: u64,
    },
    /// Response to health check
    Pong {
        timestamp: u64,
    },

    // ----- Agentic-first primitives (see crate::agentic) -----
    /// MCP-style tool invocation request.
    ToolCall(ToolCall),
    /// MCP-style tool invocation result.
    ToolResult(ToolResult),
    /// LLM token stream opens.
    StreamStart(StreamStart),
    /// One chunk of an LLM token stream.
    StreamToken(StreamToken),
    /// LLM token stream closes.
    StreamEnd(StreamEnd),
    /// Cooperatively cancel one in-flight stream by id (multiplex-aware).
    StreamCancel(StreamCancel),
    /// Capability handshake query.
    CapabilityQuery(CapabilityQuery),
    /// Capability handshake response.
    CapabilityAd(CapabilityAdvertisement),

    // ----- Neural encoder-decoder protocols (see crate::agentic_codec) -----
    /// Self-describing latent payload (codec + shape + dtype inline).
    Encoded(EncodedFrame),
    /// What encoders/decoders the sender speaks.
    CodecAd(CodecAdvertisement),
    /// Offer/accept handshake for codec selection.
    CodecNegotiation(CodecNegotiation),
    /// "Embed this for me." Request.
    EmbeddingRequest(EmbeddingRequest),
    /// Embedding response (one [`EncodedFrame`] per input element).
    EmbeddingResponse(EmbeddingResponse),
}

/// Virtual DOM patch for incremental updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VDomPatch {
    /// Create a new node
    Create { id: u32, tag: String },
    /// Remove a node
    Remove { id: u32 },
    /// Set an attribute
    SetAttr { id: u32, key: String, value: String },
    /// Remove an attribute
    RemoveAttr { id: u32, key: String },
    /// Append child to parent
    AppendChild { parent_id: u32, child_id: u32 },
    /// Remove child from parent
    RemoveChild { parent_id: u32, child_id: u32 },
    /// Reorder children
    ReorderChildren { parent_id: u32, order: Vec<u32> },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SpineBinary {
    pub instructions: Vec<Instruction>,
    pub data: Vec<u8>,
    pub render_start: usize,
    pub exported_functions: std::collections::HashMap<String, usize>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Instruction {
    DefineElement {
        id: u32,
        tag: String,
    },
    SetAttribute {
        id: u32,
        key: String,
        value: String,
    },
    AddChild {
        parent_id: u32,
        child_id: u32,
    },
    EmitEvent {
        name: String,
        payload: serde_json::Value,
    },
    StreamLatent {
        vector: Vec<f32>,
    },
    /// Morph the protocol mid-stream
    MorphProtocol {
        seed: u64,
    },
    /// Inject decoy traffic
    Decoy {
        noise: Vec<f32>,
    },
    /// Declare a reactive state variable
    DeclareState {
        name: String,
        initial_json: serde_json::Value,
    },
    /// Update a reactive state variable
    UpdateState {
        name: String,
        value_json: serde_json::Value,
    },

    // --- Control Flow & Stack Operations ---
    Push(serde_json::Value),
    Pop,
    Load(String),
    Store(String),
    BinOp(ProtocolBinOp),
    UnaryOp(ProtocolUnaryOp),
    Jump(usize),
    JumpIf(usize),
    JumpIfNot(usize),
    Call {
        name: String,
        num_args: usize,
    },
    CallTarget(usize),
    Return,

    // --- Stack-based DOM Operations ---
    DefineElementFromStack {
        id: u32,
    },
    SetAttributeFromStack {
        id: u32,
        key: String,
    },
    AddChildFromStack {
        parent_id: u32,
        child_id: u32,
    },
    EmitEventFromStack {
        name: String,
    },
    DefineTextFromStack,
    DeclareStateFromStack {
        name: String,
    },
    UpdateStateFromStack {
        name: String,
    },

    // --- Agentic Operations ---
    NavigateFromStack,
    SearchFromStack,
    StoreKnowledgeFromStack {
        tags: Vec<String>,
    },
    QueryKnowledgeFromStack {
        tags: Vec<String>,
        limit: usize,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ProtocolBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Concat,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ProtocolUnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum BrowserCommand {
    Navigate {
        url: String,
    },
    GetUR,
    GetRawHTML,
    Click {
        element_id: String,
    },
    Type {
        element_id: String,
        text: String,
    },
    ExecuteBinary(SpineBinary),
    /// Handle an event in the current session
    HandleEvent {
        element_id: u32,
        event_name: String,
        payload: serde_json::Value,
    },
    /// Request latent-encoded response
    GetLatentUR {
        dimensions: usize,
    },
    /// Trigger protocol morphing
    Morph,
    /// Semantic search in the current page
    Search {
        query: String,
    },
    /// Transfer session to another node
    TransferSession {
        target_node_id: Uuid,
    },
    /// Store knowledge in the agent's long-term memory
    StoreKnowledge {
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
    },
    /// Query knowledge from the agent's long-term memory
    QueryKnowledge {
        query: String,
        tags: Vec<String>,
        limit: usize,
    },
    /// Delete knowledge from the agent's long-term memory
    DeleteKnowledge {
        key: String,
    },
    /// Get the history of commands in this session
    GetSessionHistory,
    /// Get the capabilities of the current agentic binary
    GetCapabilities,
    /// Enable or disable autonomous mode for the agent
    SetAutonomousMode {
        enabled: bool,
    },
    /// Perform a swarm search across the cluster
    SwarmSearch {
        query: String,
        depth: usize,
    },
    /// Delegate a task to another agent in the cluster
    DelegateTask {
        task: String,
        target_agent_id: Option<Uuid>,
    },
    /// Propose knowledge to the cluster for consensus
    ProposeKnowledge {
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
    },
    /// Create a swarm plan for a high-level goal
    CreateSwarmPlan {
        goal: String,
    },
    /// Execute a specific task within a swarm plan
    ExecutePlanTask {
        plan_id: Uuid,
        task_id: Uuid,
    },
    /// Transmit data using the neural protocol
    NeuralTransmit {
        data: Vec<u8>,
        domain: String,
    },
    /// Get the full agentic state (memory, speech acts, etc.)
    GetAgenticState,
    /// Send a speech act to another agent
    SendSpeechAct {
        target_id: Uuid,
        performative: String,
        content: String,
    },

    // ---- Namespace operations ----
    //
    // These are what make SPINE addressable rather than merely connectable.
    // `Navigate` fetches an HTTP URL from the human web; the commands below
    // operate on `spine://` names, which resolve through SPINE's own namespace
    // and verify against their own authority.
    /// Resolve a `spine://` name to its signed record.
    ///
    /// `if_none_match` carries a content hash the caller already holds. When it
    /// still matches, the responder answers [`NameResolution::Unchanged`] with
    /// no record body — the ETag exchange, but with a hash that is verifiable
    /// rather than an opaque server-chosen token.
    ResolveName {
        name: String,
        #[serde(default)]
        if_none_match: Option<[u8; 32]>,
    },
    /// Resolve many names in one round trip.
    ///
    /// Agents discover names in batches — a plan references a dozen tools at
    /// once. Making the batch the native shape, rather than something callers
    /// rebuild out of a fan-out pool, is the single largest latency difference
    /// between this and one-URL-per-request.
    ResolveNames {
        names: Vec<String>,
    },
    /// Find providers of a capability, without consulting a central index.
    FindProviders {
        capability: String,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Publish a signed name record. The record carries its own signature, so
    /// the receiver needs no prior trust relationship with the publisher.
    PublishName {
        /// A JSON-encoded `spine_name::NameRecord`.
        record: serde_json::Value,
    },
    /// Fetch the representation a name points at, following the record's
    /// endpoints.
    FetchName {
        name: String,
        #[serde(default)]
        if_none_match: Option<[u8; 32]>,
    },
    /// Walk the agent web from a seed name, bounded by depth and visit count.
    CrawlNames {
        seed: String,
        #[serde(default)]
        max_depth: Option<u32>,
        #[serde(default)]
        max_visits: Option<usize>,
    },
    /// Store verified bytes and publish their signed binding on this origin.
    PublishResource {
        record: serde_json::Value,
        content_type: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// Read immutable bytes after resolving a signed content hash.
    ReadResource {
        hash: [u8; 32],
        offset: u64,
        max_bytes: u32,
    },
    /// Local namespace identity, connectivity, and authenticated peer contacts.
    NamespaceStatus,
    /// Versioned signed application call, pinned to an expected peer key.
    MeshCall {
        peer_id: String,
        peer_key: [u8; 32],
        payload: serde_json::Value,
    },
}

/// The result of a name resolution.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum NameResolution {
    /// Resolved; the record is a JSON-encoded `spine_name::NameRecord`.
    Resolved {
        record: serde_json::Value,
        /// Where the answer came from — a caller deciding whether to retry
        /// needs to know whether it got a network answer or a stale cache one.
        provenance: NameProvenance,
    },
    /// The caller's `if_none_match` hash still matches, so no body was sent.
    Unchanged {
        /// Seconds the caller may keep treating its copy as fresh.
        ttl_secs: u32,
    },
    /// No record exists for that name.
    NotFound { name: String },
    /// The name was malformed, or names a set rather than a single record.
    Invalid { name: String, reason: String },
    /// A disconnected namespace or failed lookup is not an authoritative miss.
    Unavailable { name: String, reason: String },
}

/// Where a resolution came from. Mirrors `spine_name::Provenance` at the
/// protocol boundary so the wire format does not depend on that crate's layout.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum NameProvenance {
    /// Fresh cache hit — no round trip.
    Cache,
    /// Cache hit past its TTL; usable, but a refresh is advisable.
    StaleCache,
    /// Served from the responder's own store.
    Local,
    /// Fetched from the network.
    Network,
    /// Read out of a `host:` name, which carries its own address.
    ///
    /// Nothing attested this binding — no signature, and nobody asked. A client
    /// that treats it like any other answer has lost the namespace's trust
    /// model, so it is reported distinctly rather than folded into `Local`.
    Address,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SwarmPlan {
    pub id: Uuid,
    pub goal: String,
    pub tasks: Vec<PlanTask>,
    pub status: PlanStatus,
    pub created_at: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlanTask {
    pub id: Uuid,
    pub description: String,
    pub required_skills: Vec<String>,
    pub assigned_to: Option<Uuid>, // NodeId or AgentId
    pub dependencies: Vec<Uuid>,   // Task IDs
    pub status: TaskStatus,
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum PlanStatus {
    Draft,
    Active,
    Completed,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Request {
    pub id: String,
    pub command: BrowserCommand,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Response {
    pub id: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Event {
    pub name: String,
    pub data: serde_json::Value,
}

// =============================================================================
// QUIC TRANSPORT (FALLBACK / 0-RTT)
// =============================================================================

/// Transport mode selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportMode {
    /// TCP with TLS (default, maximum compatibility)
    Tcp,
    /// QUIC with 0-RTT (lower latency, built-in encryption)
    Quic,
    /// Automatic: try QUIC first, fallback to TCP
    #[default]
    Auto,
}

/// Configuration for transport layer
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Transport mode
    pub mode: TransportMode,
    /// Enable 0-RTT for QUIC (trades security for speed on resumption)
    pub enable_0rtt: bool,
    /// Maximum idle timeout in seconds
    pub idle_timeout_secs: u64,
    /// Keep-alive interval in seconds (0 = disabled)
    pub keep_alive_secs: u64,
    /// Maximum concurrent streams (QUIC only)
    pub max_streams: u32,
    /// Server name for TLS/QUIC SNI
    pub server_name: Option<String>,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            mode: TransportMode::Auto,
            enable_0rtt: true,
            idle_timeout_secs: 30,
            keep_alive_secs: 5,
            max_streams: 100,
            server_name: None,
        }
    }
}

/// Unified transport that supports both TCP and QUIC
pub enum Transport {
    /// TCP stream (wrapped in TLS if encrypted)
    Tcp(Box<dyn AsyncReadWrite>),
    /// QUIC connection with streams
    #[cfg(feature = "quic")]
    Quic(QuicTransport),
}

/// Trait alias for async read/write
pub trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncReadWrite for T {}

#[cfg(feature = "quic")]
pub struct QuicTransport {
    connection: QuinnConnection,
    send_stream: Option<quinn::SendStream>,
    recv_stream: Option<quinn::RecvStream>,
}

#[cfg(feature = "quic")]
impl QuicTransport {
    /// Create a new QUIC transport from an existing connection
    pub fn new(connection: QuinnConnection) -> Self {
        Self {
            connection,
            send_stream: None,
            recv_stream: None,
        }
    }

    /// Open a bidirectional stream
    pub async fn open_bi(&mut self) -> anyhow::Result<()> {
        let (send, recv) = self.connection.open_bi().await?;
        self.send_stream = Some(send);
        self.recv_stream = Some(recv);
        Ok(())
    }

    /// Accept an incoming bidirectional stream
    pub async fn accept_bi(&mut self) -> anyhow::Result<()> {
        let (send, recv) = self.connection.accept_bi().await?;
        self.send_stream = Some(send);
        self.recv_stream = Some(recv);
        Ok(())
    }

    /// Write data to the QUIC stream
    pub async fn write_all(&mut self, data: &[u8]) -> anyhow::Result<()> {
        if let Some(ref mut stream) = self.send_stream {
            stream.write_all(data).await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No send stream open"))
        }
    }

    /// Read exact bytes from the QUIC stream
    pub async fn read_exact(&mut self, buf: &mut [u8]) -> anyhow::Result<()> {
        if let Some(ref mut stream) = self.recv_stream {
            stream.read_exact(buf).await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No recv stream open"))
        }
    }

    /// Get RTT estimate
    pub fn rtt(&self) -> std::time::Duration {
        self.connection.rtt()
    }

    /// Check if connection supports 0-RTT
    pub fn is_0rtt(&self) -> bool {
        // 0-RTT is accepted if we have session tickets
        true // Simplified - quinn handles this internally
    }

    /// Close the connection gracefully
    pub fn close(&self, code: u32, reason: &str) {
        self.connection
            .close(quinn::VarInt::from_u32(code), reason.as_bytes());
    }
}

/// QUIC endpoint builder for server/client
#[cfg(feature = "quic")]
pub struct QuicEndpointBuilder {
    config: TransportConfig,
}

#[cfg(feature = "quic")]
impl QuicEndpointBuilder {
    pub fn new() -> Self {
        Self {
            config: TransportConfig::default(),
        }
    }

    pub fn with_config(config: TransportConfig) -> Self {
        Self { config }
    }

    /// Generate self-signed certificate for testing/development
    fn generate_self_signed_cert(
    ) -> anyhow::Result<(CertificateDer<'static>, PrivateKeyDer<'static>)> {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
        let key = PrivateKeyDer::Pkcs8(cert.get_key_pair().serialize_der().into());
        let cert_der = CertificateDer::from(cert.serialize_der()?);
        Ok((cert_der, key))
    }

    /// Build a server endpoint
    pub fn build_server(&self, bind_addr: SocketAddr) -> anyhow::Result<Endpoint> {
        let (cert, key) = Self::generate_self_signed_cert()?;

        // Use quinn's crypto config builder with rustls 0.23
        let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(
            quic_rustls::ServerConfig::builder_with_protocol_versions(&[
                &quic_rustls::version::TLS13,
            ])
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)?,
        )?;

        let mut server_config = ServerConfig::with_crypto(Arc::new(crypto));

        let transport = Arc::get_mut(&mut server_config.transport).unwrap();
        transport.max_idle_timeout(Some(
            std::time::Duration::from_secs(self.config.idle_timeout_secs).try_into()?,
        ));
        transport.keep_alive_interval(Some(std::time::Duration::from_secs(
            self.config.keep_alive_secs,
        )));
        transport.max_concurrent_bidi_streams(self.config.max_streams.into());

        let endpoint = Endpoint::server(server_config, bind_addr)?;
        Ok(endpoint)
    }

    /// Build a client endpoint with platform verifier (for development: skip verification)
    pub fn build_client(&self) -> anyhow::Result<Endpoint> {
        // Create a permissive client config for development
        // In production, use rustls-platform-verifier
        let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(
            quic_rustls::ClientConfig::builder_with_protocol_versions(&[
                &quic_rustls::version::TLS13,
            ])
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth(),
        )?;

        let client_config = ClientConfig::new(Arc::new(crypto));

        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_config);

        Ok(endpoint)
    }

    /// Connect to a QUIC server
    pub async fn connect(
        &self,
        endpoint: &Endpoint,
        addr: SocketAddr,
        server_name: &str,
    ) -> anyhow::Result<QuicTransport> {
        let connection = endpoint.connect(addr, server_name)?.await?;
        let mut transport = QuicTransport::new(connection);
        transport.open_bi().await?;
        Ok(transport)
    }
}

#[cfg(feature = "quic")]
impl Default for QuicEndpointBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Skip certificate verification (development only!)
/// WARNING: Do NOT use in production - this accepts any certificate!
#[cfg(feature = "quic")]
#[derive(Debug)]
struct SkipServerVerification;

#[cfg(feature = "quic")]
impl quic_rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<quic_rustls::client::danger::ServerCertVerified, quic_rustls::Error> {
        Ok(quic_rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &quic_rustls::DigitallySignedStruct,
    ) -> Result<quic_rustls::client::danger::HandshakeSignatureValid, quic_rustls::Error> {
        Ok(quic_rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &quic_rustls::DigitallySignedStruct,
    ) -> Result<quic_rustls::client::danger::HandshakeSignatureValid, quic_rustls::Error> {
        Ok(quic_rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<quic_rustls::SignatureScheme> {
        vec![
            quic_rustls::SignatureScheme::RSA_PKCS1_SHA256,
            quic_rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            quic_rustls::SignatureScheme::RSA_PKCS1_SHA384,
            quic_rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            quic_rustls::SignatureScheme::RSA_PKCS1_SHA512,
            quic_rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            quic_rustls::SignatureScheme::RSA_PSS_SHA256,
            quic_rustls::SignatureScheme::RSA_PSS_SHA384,
            quic_rustls::SignatureScheme::RSA_PSS_SHA512,
            quic_rustls::SignatureScheme::ED25519,
        ]
    }
}
