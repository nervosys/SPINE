// Allow dead code for protocol features designed for future extensions
#![allow(dead_code)]

pub mod agentic;
pub mod body;
pub mod cbor;
pub mod connection;
pub mod frame;
pub mod hello;
pub mod mcp;
pub mod native;
pub mod replay;
pub mod resource;
pub mod tasks;
pub mod wirefmt;

pub use agentic::{
    Capability, CapabilityAdvertisement, CapabilityQuery, CapabilitySelector, DecodeHints,
    StreamCancel, StreamData, StreamEnd, StreamEndReason, StreamRole, StreamStart, StreamToken,
    StreamUsage, ToolCall, ToolOutcome, TraceContext,
};
pub use body::{Reply, WireError};
use wirefmt::reject_null;

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
// MESSAGE TYPES
// =============================================================================

/// A decoded frame body, for traces ([`replay`]) and logs.
///
/// This is **not** a wire body. A `REQUEST` body is a [`BrowserCommand`], a
/// `RESPONSE` body is a [`Reply`], and a `PING` or `PONG` body is an unsigned
/// integer token; none of them is wrapped in a `Message` on the wire. The
/// variants exist so a recorded session can hold any of them in one type, and
/// so [`native::decode_stream_body`] can return the body of whichever
/// `STREAM_*` frame arrived. Nothing in the workspace produces a stream.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum Message {
    /// A `REQUEST` body.
    #[serde(rename = "command")]
    Command(BrowserCommand),
    /// A `RESPONSE` body; the result is the JSON-shaped value of D.
    #[serde(rename = "reply")]
    Reply(Reply<serde_json::Value>),
    /// A `PING` token.
    #[serde(rename = "ping")]
    Ping { timestamp: u64 },
    /// A `PONG` token.
    #[serde(rename = "pong")]
    Pong { timestamp: u64 },
    /// A `STREAM_START` body.
    #[serde(rename = "stream_start")]
    StreamStart(StreamStart),
    /// A `STREAM_TOKEN` body.
    #[serde(rename = "stream_token")]
    StreamToken(StreamToken),
    /// A `STREAM_END` body.
    #[serde(rename = "stream_end")]
    StreamEnd(StreamEnd),
    /// A `STREAM_CANCEL` body.
    #[serde(rename = "stream_cancel")]
    StreamCancel(StreamCancel),
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

/// A compiled HLB program: the argument of `execute_binary`.
///
/// A closed struct. `data` is a CBOR byte string. `exported_functions` is a
/// [`BTreeMap`](std::collections::BTreeMap) so two encodings of one program are
/// identical whatever the insertion order (the encoder sorts map keys anyway,
/// but the type now says so).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SpineBinary {
    pub instructions: Vec<Instruction>,
    #[serde(with = "wirefmt::bytes")]
    pub data: Vec<u8>,
    pub render_start: usize,
    pub exported_functions: std::collections::BTreeMap<String, usize>,
    pub capabilities: Vec<String>,
}

/// One HLB instruction. Wire names are the snake_case names in the
/// `rename` attributes; they are spelled out because `rename_all` would
/// split acronyms and numerals unpredictably.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub enum Instruction {
    #[serde(rename = "define_element")]
    DefineElement { id: u32, tag: String },
    #[serde(rename = "set_attribute")]
    SetAttribute { id: u32, key: String, value: String },
    #[serde(rename = "add_child")]
    AddChild { parent_id: u32, child_id: u32 },
    #[serde(rename = "emit_event")]
    EmitEvent {
        name: String,
        payload: serde_json::Value,
    },
    #[serde(rename = "stream_latent")]
    StreamLatent { vector: Vec<f32> },
    /// Morph the protocol mid-stream
    #[serde(rename = "morph_protocol")]
    MorphProtocol { seed: u64 },
    /// Inject decoy traffic
    #[serde(rename = "decoy")]
    Decoy { noise: Vec<f32> },
    /// Declare a reactive state variable
    #[serde(rename = "declare_state")]
    DeclareState {
        name: String,
        initial_json: serde_json::Value,
    },
    /// Update a reactive state variable
    #[serde(rename = "update_state")]
    UpdateState {
        name: String,
        value_json: serde_json::Value,
    },

    // --- Control Flow & Stack Operations ---
    #[serde(rename = "push")]
    Push(serde_json::Value),
    #[serde(rename = "pop")]
    Pop,
    #[serde(rename = "load")]
    Load(String),
    #[serde(rename = "store")]
    Store(String),
    #[serde(rename = "bin_op")]
    BinOp(ProtocolBinOp),
    #[serde(rename = "unary_op")]
    UnaryOp(ProtocolUnaryOp),
    #[serde(rename = "jump")]
    Jump(usize),
    #[serde(rename = "jump_if")]
    JumpIf(usize),
    #[serde(rename = "jump_if_not")]
    JumpIfNot(usize),
    #[serde(rename = "call")]
    Call { name: String, num_args: usize },
    #[serde(rename = "call_target")]
    CallTarget(usize),
    #[serde(rename = "return")]
    Return,

    // --- Stack-based DOM Operations ---
    #[serde(rename = "define_element_from_stack")]
    DefineElementFromStack { id: u32 },
    #[serde(rename = "set_attribute_from_stack")]
    SetAttributeFromStack { id: u32, key: String },
    #[serde(rename = "add_child_from_stack")]
    AddChildFromStack { parent_id: u32, child_id: u32 },
    #[serde(rename = "emit_event_from_stack")]
    EmitEventFromStack { name: String },
    #[serde(rename = "define_text_from_stack")]
    DefineTextFromStack,
    #[serde(rename = "declare_state_from_stack")]
    DeclareStateFromStack { name: String },
    #[serde(rename = "update_state_from_stack")]
    UpdateStateFromStack { name: String },

    // --- Agentic Operations ---
    #[serde(rename = "navigate_from_stack")]
    NavigateFromStack,
    #[serde(rename = "search_from_stack")]
    SearchFromStack,
    #[serde(rename = "store_knowledge_from_stack")]
    StoreKnowledgeFromStack { tags: Vec<String> },
    #[serde(rename = "query_knowledge_from_stack")]
    QueryKnowledgeFromStack { tags: Vec<String>, limit: usize },
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolBinOp {
    #[serde(rename = "add")]
    Add,
    #[serde(rename = "sub")]
    Sub,
    #[serde(rename = "mul")]
    Mul,
    #[serde(rename = "div")]
    Div,
    #[serde(rename = "mod")]
    Mod,
    #[serde(rename = "eq")]
    Eq,
    #[serde(rename = "ne")]
    Ne,
    #[serde(rename = "lt")]
    Lt,
    #[serde(rename = "le")]
    Le,
    #[serde(rename = "gt")]
    Gt,
    #[serde(rename = "ge")]
    Ge,
    #[serde(rename = "and")]
    And,
    #[serde(rename = "or")]
    Or,
    #[serde(rename = "concat")]
    Concat,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolUnaryOp {
    #[serde(rename = "not")]
    Not,
    #[serde(rename = "neg")]
    Neg,
}

/// The closed v1 command set: the body of a `REQUEST` (C.5 rule 7).
///
/// A command with fields is a single-entry map `{name: fields}`; a command
/// without is the text string of its name. The 17 browser commands, plus
/// `tool_call` and `capability_query` from D.1, are the whole set; a name
/// outside it is answered with `unknown_command`. The Rust name is historical:
/// it now holds more than browser commands.
///
/// Wire names are the `rename` attributes (listed in
/// `docs/src/architecture/protocol.md`); [`COMMAND_NAMES`] lists them in
/// declaration order.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub enum BrowserCommand {
    /// Fetch an HTTP URL from the human web into the session.
    #[serde(rename = "navigate")]
    Navigate { url: String },
    /// The unified representation of the loaded page.
    #[serde(rename = "get_ur")]
    GetUR,
    /// The loaded page's HTML.
    #[serde(rename = "get_raw_html")]
    GetRawHTML,
    /// Run an HLB program in the session.
    #[serde(rename = "execute_binary")]
    ExecuteBinary(SpineBinary),
    /// Store knowledge in the agent's long-term memory
    #[serde(rename = "store_knowledge")]
    StoreKnowledge {
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
    },
    /// Query knowledge from the agent's long-term memory
    #[serde(rename = "query_knowledge")]
    QueryKnowledge {
        query: String,
        tags: Vec<String>,
        limit: usize,
    },
    /// Get the capabilities of the current agentic binary
    #[serde(rename = "get_binary_capabilities")]
    GetBinaryCapabilities,

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
    #[serde(rename = "resolve_name")]
    ResolveName {
        name: String,
        #[serde(
            default,
            with = "wirefmt::opt_byte_array",
            skip_serializing_if = "Option::is_none"
        )]
        if_none_match: Option<[u8; 32]>,
    },
    /// Resolve many names in one round trip.
    ///
    /// Agents discover names in batches — a plan references a dozen tools at
    /// once. Making the batch the native shape, rather than something callers
    /// rebuild out of a fan-out pool, is the single largest latency difference
    /// between this and one-URL-per-request.
    #[serde(rename = "resolve_names")]
    ResolveNames { names: Vec<String> },
    /// Find providers of a capability, without consulting a central index.
    #[serde(rename = "find_providers")]
    FindProviders {
        capability: String,
        #[serde(
            default,
            deserialize_with = "reject_null",
            skip_serializing_if = "Option::is_none"
        )]
        limit: Option<usize>,
    },
    /// Publish a signed name record. The record carries its own signature, so
    /// the receiver needs no prior trust relationship with the publisher.
    #[serde(rename = "publish_name")]
    PublishName {
        /// A JSON-encoded `spine_name::NameRecord`.
        record: serde_json::Value,
    },
    /// Fetch the representation a name points at, following the record's
    /// endpoints.
    #[serde(rename = "fetch_name")]
    FetchName {
        name: String,
        #[serde(
            default,
            with = "wirefmt::opt_byte_array",
            skip_serializing_if = "Option::is_none"
        )]
        if_none_match: Option<[u8; 32]>,
    },
    /// Walk the agent web from a seed name, bounded by depth and visit count.
    #[serde(rename = "crawl_names")]
    CrawlNames {
        seed: String,
        #[serde(
            default,
            deserialize_with = "reject_null",
            skip_serializing_if = "Option::is_none"
        )]
        max_depth: Option<u32>,
        #[serde(
            default,
            deserialize_with = "reject_null",
            skip_serializing_if = "Option::is_none"
        )]
        max_visits: Option<usize>,
    },
    /// Store verified bytes and publish their signed binding on this origin.
    #[serde(rename = "publish_resource")]
    PublishResource {
        record: serde_json::Value,
        content_type: String,
        #[serde(with = "wirefmt::bytes")]
        data: Vec<u8>,
    },
    /// Read immutable bytes after resolving a signed content hash.
    #[serde(rename = "read_resource")]
    ReadResource {
        #[serde(with = "wirefmt::byte_array")]
        hash: [u8; 32],
        offset: u64,
        max_bytes: u32,
    },
    /// Local namespace identity, connectivity, and authenticated peer contacts.
    #[serde(rename = "namespace_status")]
    NamespaceStatus,
    /// Versioned signed application call, pinned to an expected peer key.
    #[serde(rename = "mesh_call")]
    MeshCall {
        peer_id: String,
        #[serde(with = "wirefmt::byte_array")]
        peer_key: [u8; 32],
        payload: serde_json::Value,
    },

    // ---- From D.1: the agentic primitives that are commands.
    /// Invoke a tool: `{name, args[, trace]}`. The reply's `ok` is the tool's
    /// content; a tool failure is an `err`.
    #[serde(rename = "tool_call")]
    ToolCall(ToolCall),
    /// Ask what this origin offers: `{selector}`. The reply's `ok` is a
    /// [`CapabilityAdvertisement`].
    #[serde(rename = "capability_query")]
    CapabilityQuery(CapabilityQuery),
}

/// The wire names of every [`BrowserCommand`], in declaration order. The
/// daemon uses it to tell `unknown_command` from `invalid_request`.
pub const COMMAND_NAMES: [&str; 19] = [
    "navigate",
    "get_ur",
    "get_raw_html",
    "execute_binary",
    "store_knowledge",
    "query_knowledge",
    "get_binary_capabilities",
    "resolve_name",
    "resolve_names",
    "find_providers",
    "publish_name",
    "fetch_name",
    "crawl_names",
    "publish_resource",
    "read_resource",
    "namespace_status",
    "mesh_call",
    "tool_call",
    "capability_query",
];

/// The result of a name resolution.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub enum NameResolution {
    /// Resolved; the record is a JSON-encoded `spine_name::NameRecord`.
    #[serde(rename = "resolved")]
    Resolved {
        record: serde_json::Value,
        /// Where the answer came from — a caller deciding whether to retry
        /// needs to know whether it got a network answer or a stale cache one.
        provenance: NameProvenance,
    },
    /// The caller's `if_none_match` hash still matches, so no body was sent.
    #[serde(rename = "unchanged")]
    Unchanged {
        /// Seconds the caller may keep treating its copy as fresh.
        ttl_secs: u32,
    },
    /// No record exists for that name.
    #[serde(rename = "not_found")]
    NotFound { name: String },
    /// The name was malformed, or names a set rather than a single record.
    #[serde(rename = "invalid")]
    Invalid { name: String, reason: String },
    /// A disconnected namespace or failed lookup is not an authoritative miss.
    #[serde(rename = "unavailable")]
    Unavailable { name: String, reason: String },
}

/// Where a resolution came from. Mirrors `spine_name::Provenance` at the
/// protocol boundary so the wire format does not depend on that crate's layout.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum NameProvenance {
    /// Fresh cache hit — no round trip.
    #[serde(rename = "cache")]
    Cache,
    /// Cache hit past its TTL; usable, but a refresh is advisable.
    #[serde(rename = "stale_cache")]
    StaleCache,
    /// Served from the responder's own store.
    #[serde(rename = "local")]
    Local,
    /// Fetched from the network.
    #[serde(rename = "network")]
    Network,
    /// Read out of a `host:` name, which carries its own address.
    ///
    /// Nothing attested this binding — no signature, and nobody asked. A client
    /// that treats it like any other answer has lost the namespace's trust
    /// model, so it is reported distinctly rather than folded into `Local`.
    #[serde(rename = "address")]
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
