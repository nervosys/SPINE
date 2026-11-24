// Allow fire-and-forget async patterns for distributed operations
#![allow(clippy::let_underscore_future)]

use anyhow::{Context, Result};
use axum::{routing::get, Router};
use dashmap::DashMap;
use prometheus::{Encoder, TextEncoder};
use serde::{Deserialize, Serialize};
#[cfg(feature = "research")]
use spine_cluster::{ClusterNode, NodeCapabilities};
use spine_core::outbound;
use spine_protocol::body::{
    ERR_BATCH_TOO_LARGE, ERR_CAPABILITY_DENIED, ERR_EXECUTION_FAILED, ERR_INTERNAL,
    ERR_INVALID_PEER_ID, ERR_INVALID_REQUEST, ERR_INVALID_SEED, ERR_LOOKUP_FAILED,
    ERR_LOOKUP_UNAVAILABLE, ERR_MALFORMED_RECORD, ERR_MESH_CALL_FAILED, ERR_NAMESPACE_UNAVAILABLE,
    ERR_NO_PAGE, ERR_OVERLOADED, ERR_PARSE_ERROR, ERR_PUBLISH_FAILED, ERR_RATE_LIMIT,
    ERR_RECORD_EXPIRED, ERR_RECORD_REJECTED, ERR_RESOURCE_NOT_FOUND, ERR_SESSION_CAPACITY,
    ERR_SESSION_EXPIRED, ERR_SESSION_NOT_FOUND,
};
use spine_protocol::connection::{
    response_ok_body, Connection, ConnectionError, ConnectionReader, ConnectionWriter, Incoming,
};
use spine_protocol::frame::{CloseCode, Frame, FrameType};
use spine_protocol::native;
use spine_protocol::{BrowserCommand, ToolOutcome, WireError};
use spine_transport::WebSocketStream;
use spine_wasm::WasmRuntime;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::{debug, error, info, instrument, span, warn, Instrument, Level};
use uuid::Uuid;

mod config;
#[allow(dead_code)]
mod ct;
mod namespace;
mod telemetry;
#[allow(dead_code)]
mod tls;
mod vdom;
use config::SpineConfig;
use telemetry::*;
use tls::*;
use vdom::VirtualDom;

#[derive(Serialize, Deserialize, Clone)]
struct Session {
    current_url: Option<String>,
    current_html: Option<String>,
    /// Cached VDOM from last ExecuteBinary
    current_vdom: Option<VirtualDom>,
    /// Cached parsed UnifiedRepresentation (invalidated on navigation)
    #[serde(skip)]
    cached_ur: Option<spine_parser::UnifiedRepresentation>,
    /// Reactive state variables
    state: std::collections::HashMap<String, serde_json::Value>,
    /// Current HLB binary for reactivity
    current_binary: Option<spine_protocol::SpineBinary>,
    /// Last detach/import time. Zero is a legacy record granted one retention interval.
    #[serde(default)]
    retained_at: u64,
    #[serde(skip)]
    detached_since: Option<std::time::Instant>,
}

impl Session {
    fn new() -> Self {
        Self {
            current_url: None,
            current_html: None,
            current_vdom: None,
            cached_ur: None,
            state: std::collections::HashMap::new(),
            current_binary: None,
            retained_at: unix_now(),
            detached_since: Some(std::time::Instant::now()),
        }
    }
}

/// Connections own this lease across awaited commands; aborts also detach the session.
struct RateLease {
    state: Arc<BrowserState>,
    id: String,
}
impl Drop for RateLease {
    fn drop(&mut self) {
        self.state.rate_limits.remove(&self.id);
    }
}

struct SessionLease {
    state: Arc<BrowserState>,
    id: String,
}
impl Drop for SessionLease {
    fn drop(&mut self) {
        let mut active = self.state.session_gate.lock().unwrap();
        if let Some(mut session) = self.state.sessions.get_mut(&self.id) {
            session.retained_at = unix_now();
            session.detached_since = Some(std::time::Instant::now());
        }
        active.remove(&self.id);
        self.state.rate_limits.remove(&self.id);
    }
}

#[derive(Serialize, Deserialize)]
struct SessionSnapshot {
    version: u32,
    sessions: std::collections::BTreeMap<String, Session>,
    knowledge: std::collections::BTreeMap<String, Vec<KnowledgeEntry>>,
}

#[derive(Serialize, Deserialize, Clone)]
struct KnowledgeEntry {
    key: String,
    value: serde_json::Value,
    tags: Vec<String>,
    timestamp_ns: u64,
}

#[cfg(feature = "research")]
#[derive(Clone)]
struct KnowledgeProposal {
    key: String,
    value: serde_json::Value,
    tags: Vec<String>,
    votes: Vec<KnowledgeVote>,
    origin_node: spine_cluster::NodeId,
}

#[cfg(feature = "research")]
#[derive(Clone)]
struct KnowledgeVote {
    voter_id: spine_cluster::NodeId,
    approved: bool,
    confidence: f32,
}

struct BrowserState {
    resources: spine_core::resources::ResourceStore,
    sessions: DashMap<String, Session>,
    /// Serializes admission/import/reaping and protects connected sessions.
    session_gate: std::sync::Mutex<std::collections::HashSet<String>>,
    snapshot_gate: Mutex<()>,
    knowledge_base: DashMap<String, Vec<KnowledgeEntry>>,
    #[cfg(feature = "research")]
    proposals: DashMap<Uuid, KnowledgeProposal>,
    #[cfg(feature = "research")]
    plans: DashMap<Uuid, spine_protocol::SwarmPlan>,
    fetcher: outbound::Fetcher,
    #[cfg(feature = "research")]
    cluster: Mutex<ClusterNode>,
    /// Rate limiter: session_id -> (tokens, last_update)
    rate_limits: DashMap<String, (f64, std::time::Instant)>,
    /// Cached WasmRuntime (expensive to create, safe to reuse). Shared
    /// by `Arc` so a program can run on a blocking thread.
    wasm_runtime: Arc<spine_wasm::WasmRuntime>,
    /// Process-wide permits for programs compiling and running at once.
    execute_permits: Arc<tokio::sync::Semaphore>,
    /// Shared native connection admission, including unfinished handshakes.
    connections: Arc<spine_core::connections::ConnectionBudget>,
    /// Server start time for uptime calculation
    start_time: std::time::Instant,
    /// Shutdown signal: when true, stop accepting new connections
    shutting_down: std::sync::atomic::AtomicBool,
    /// Becomes true when shutdown starts. Each native session watches it, sends
    /// `CLOSE(shutting_down)` once it is idle, and ends.
    shutdown: tokio::sync::watch::Sender<bool>,
    /// Server configuration
    config: SpineConfig,
    /// The namespace this node serves: names it publishes, plus records it has
    /// learned. This is what makes the node an *origin* in the agent web rather
    /// than only a proxy onto the human one.
    names: Arc<spine_name::LocalResolver>,
    /// The mesh this node resolves *through*, when it has joined one.
    ///
    /// `None` means the node is an origin but not a participant: it serves what
    /// it holds and cannot look anything up. That is a supported configuration,
    /// not a failure — a node with no seeds has nothing to route through.
    mesh_names: Option<Arc<spine_mesh::naming_mesh::MeshNameResolver>>,
}

impl BrowserState {
    fn create_session(self: &Arc<Self>) -> Option<SessionLease> {
        let mut active = self.session_gate.lock().unwrap();
        if self.sessions.len() >= self.config.server.max_sessions {
            return None;
        }
        let id = Uuid::new_v4().to_string();
        self.sessions.insert(id.clone(), Session::new());
        active.insert(id.clone());
        Some(SessionLease {
            state: self.clone(),
            id,
        })
    }

    #[cfg(feature = "research")]
    fn import_session(&self, id: String, mut session: Session) -> bool {
        let _gate = self.session_gate.lock().unwrap();
        if !spine_core::session_store::valid_id(&id)
            || self.sessions.len() >= self.config.server.max_sessions
            || self.sessions.contains_key(&id)
        {
            return false;
        }
        session.retained_at = unix_now();
        session.detached_since = Some(std::time::Instant::now());
        self.sessions.insert(id, session);
        true
    }

    fn reap_sessions(&self) -> Vec<String> {
        let active = self.session_gate.lock().unwrap();
        let ttl = std::time::Duration::from_secs(self.config.server.session_retention_secs);
        let ids: Vec<_> = self
            .sessions
            .iter()
            .filter(|e| {
                !active.contains(e.key()) && e.detached_since.is_some_and(|t| t.elapsed() >= ttl)
            })
            .map(|e| e.key().clone())
            .collect();
        for id in &ids {
            self.sessions.remove(id);
            self.knowledge_base.remove(id);
            self.rate_limits.remove(id);
        }
        ids
    }

    async fn save_sessions(self: &Arc<Self>) -> anyhow::Result<()> {
        let _serial = self.snapshot_gate.lock().await;
        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let snapshot = {
                let active = state.session_gate.lock().unwrap();
                let now = unix_now();
                SessionSnapshot {
                    version: 1,
                    sessions: state
                        .sessions
                        .iter()
                        .map(|e| {
                            let mut session = e.value().clone();
                            if active.contains(e.key()) {
                                session.retained_at = now;
                            }
                            (e.key().clone(), session)
                        })
                        .collect(),
                    knowledge: state
                        .knowledge_base
                        .iter()
                        .map(|e| (e.key().clone(), e.value().clone()))
                        .collect(),
                }
            };
            spine_core::session_store::write(
                std::path::Path::new("spine-sessions-v1.json"),
                &snapshot,
            )
        })
        .await?
    }

    fn load_sessions(&self) -> anyhow::Result<()> {
        use spine_core::session_store::{read, valid_id, MAX_LEGACY_BYTES, MAX_SNAPSHOT_BYTES};
        let manifest = std::path::Path::new("spine-sessions-v1.json");
        let mut snapshot = if manifest.exists() {
            let snapshot: SessionSnapshot = read(manifest, MAX_SNAPSHOT_BYTES)?;
            anyhow::ensure!(
                snapshot.version == 1,
                "unsupported session snapshot version"
            );
            snapshot
        } else {
            // One-time migration. An authoritative manifest prevents old files resurrecting records.
            let mut snapshot = SessionSnapshot {
                version: 1,
                sessions: Default::default(),
                knowledge: Default::default(),
            };
            let mut total = 0u64;
            for directory in ["sessions", "knowledge"] {
                let dir = std::path::Path::new(directory);
                if !dir.exists() {
                    continue;
                }
                let mut count = 0usize;
                for entry in std::fs::read_dir(dir)? {
                    let path = entry?.path();
                    if path.extension().and_then(|s| s.to_str()) != Some("json") {
                        continue;
                    }
                    let Some(id) = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .filter(|id| valid_id(id))
                    else {
                        continue;
                    };
                    count += 1;
                    anyhow::ensure!(
                        count <= self.config.server.max_sessions + 1,
                        "legacy session record count exceeds configured limit"
                    );
                    total = total.saturating_add(std::fs::metadata(&path)?.len());
                    anyhow::ensure!(
                        total <= MAX_SNAPSHOT_BYTES,
                        "legacy session data exceeds byte limit"
                    );
                    if directory == "sessions" {
                        snapshot
                            .sessions
                            .insert(id.into(), read(&path, MAX_LEGACY_BYTES)?);
                    } else {
                        snapshot
                            .knowledge
                            .insert(id.into(), read(&path, MAX_LEGACY_BYTES)?);
                    }
                }
            }
            snapshot
        };
        anyhow::ensure!(
            snapshot.sessions.len() <= self.config.server.max_sessions,
            "persisted session count exceeds configured limit"
        );
        anyhow::ensure!(
            snapshot.knowledge.len() <= self.config.server.max_sessions + 1,
            "persisted knowledge count exceeds configured limit"
        );
        let now = unix_now();
        let ttl = self.config.server.session_retention_secs;
        snapshot.sessions.retain(|id, session| {
            if !valid_id(id) {
                return false;
            }
            let Some(age) = spine_core::session_store::restored_age(session.retained_at, now, ttl)
            else {
                return false;
            };
            session.retained_at = now.saturating_sub(age);
            session.detached_since = Some(
                std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_secs(age))
                    .unwrap_or_else(std::time::Instant::now),
            );
            true
        });
        snapshot
            .knowledge
            .retain(|id, _| id == "cluster_shared" || snapshot.sessions.contains_key(id));
        // Commit migration/expiry before exposing state or starting listeners. Fail closed on corruption.
        spine_core::session_store::write(manifest, &snapshot)?;
        for (id, session) in snapshot.sessions {
            self.sessions.insert(id, session);
        }
        for (id, knowledge) in snapshot.knowledge {
            self.knowledge_base.insert(id, knowledge);
        }
        Ok(())
    }

    fn check_rate_limit(&self, session_id: &str) -> bool {
        let max_tokens = 100.0;
        let refill_rate = 10.0; // tokens per second

        let mut entry = self
            .rate_limits
            .entry(session_id.to_string())
            .or_insert((max_tokens, std::time::Instant::now()));
        let (tokens, last_update) = entry.value_mut();

        let now = std::time::Instant::now();
        let elapsed = now.duration_since(*last_update).as_secs_f64();
        *tokens = (*tokens + elapsed * refill_rate).min(max_tokens);
        *last_update = now;

        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Wall-clock seconds since the Unix epoch, for record freshness.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Directed writes are transport delivery attempts, not storage acknowledgements.
/// Report local persistence independently and preserve partial mesh failures.
async fn publication_report(
    state: &BrowserState,
    record: spine_name::NameRecord,
    log: Option<spine_name::SuccessionLog>,
) -> serde_json::Value {
    let mut sent = 0;
    let mut failed = 0;
    let mut broadcast = false;
    let mut replication_error = None;
    if let Some(mesh) = &state.mesh_names {
        // A rotated name's record is admitted and replicated with its log.
        let outcome = match log {
            Some(log) => mesh.publish_with_log(record.clone(), log).await,
            None => mesh.publish(record.clone()).await,
        };
        match outcome {
            Ok(report) => {
                sent = report.sent;
                failed = report.failed;
                broadcast = report.broadcast;
            }
            Err(error) => replication_error = Some(error.to_string()),
        }
    }
    serde_json::json!({
        "status": "published", "name": record.name.to_string(), "stored": true,
        "local_persistent": !state.config.namespace.records_path.is_empty(),
        "replicas": sent, "replication_sent": sent, "replication_failed": failed,
        "replication_broadcast": broadcast, "replication_error": replication_error,
        "replication_acknowledged": 0, "durable": false,
        "retry": "mesh maintenance reoffers live records; remote persistence is not acknowledged"
    })
}

/// Map a resolver provenance onto its protocol-level counterpart.
fn wire_provenance(p: spine_name::Provenance) -> spine_protocol::NameProvenance {
    match p {
        spine_name::Provenance::Cache => spine_protocol::NameProvenance::Cache,
        spine_name::Provenance::StaleCache => spine_protocol::NameProvenance::StaleCache,
        spine_name::Provenance::Local => spine_protocol::NameProvenance::Local,
        spine_name::Provenance::Network => spine_protocol::NameProvenance::Network,
        spine_name::Provenance::Address => spine_protocol::NameProvenance::Address,
    }
}

/// Resolve one name, honouring a caller-supplied content-hash validator.
async fn resolve_one(
    state: &Arc<BrowserState>,
    raw: &str,
    if_none_match: Option<[u8; 32]>,
) -> spine_protocol::NameResolution {
    use spine_name::Resolver;

    let uri = match spine_name::SpineUri::parse(raw) {
        Ok(u) => u,
        Err(e) => {
            return spine_protocol::NameResolution::Invalid {
                name: raw.to_string(),
                reason: e.to_string(),
            }
        }
    };

    match state.names.resolve(&uri).await {
        Ok(resolution) => {
            // The validator exchange: if the caller already holds this exact
            // representation, say so and send no body. The hash is the content's
            // own, so unlike an ETag the caller can verify the claim itself.
            if let (Some(theirs), Some(ours)) = (if_none_match, resolution.record.content_hash) {
                if theirs == ours {
                    return spine_protocol::NameResolution::Unchanged {
                        ttl_secs: resolution.record.remaining_ttl(state.names.now()) as u32,
                    };
                }
            }
            match serde_json::to_value(&resolution.record) {
                Ok(record) => spine_protocol::NameResolution::Resolved {
                    record,
                    provenance: wire_provenance(resolution.provenance),
                },
                Err(e) => spine_protocol::NameResolution::Invalid {
                    name: raw.to_string(),
                    reason: e.to_string(),
                },
            }
        }
        // A still-valid negative entry is an answer, not a miss: it is the whole
        // point of caching the absence. Both surface as `NotFound` from the
        // resolver, so ask the cache which this was before walking the mesh.
        // Publishing or renewing a name clears the entry, so this cannot mask a
        // newly published record.
        Err(spine_name::NameError::NotFound(_)) if state.names.is_negative_cached(&uri) => {
            spine_protocol::NameResolution::NotFound {
                name: raw.to_string(),
            }
        }
        // Not held locally: this is where the node stops being a directory and
        // starts being a participant. Cache what the walk returns, so the next
        // ask for the same name is answered without another round trip.
        Err(spine_name::NameError::NotFound(_)) => match resolve_over_mesh(state, &uri).await {
            Ok(Some(record)) if record.is_tombstone() => {
                let _ = state.names.cache_record(record);
                spine_protocol::NameResolution::Invalid {
                    name: raw.to_string(),
                    reason: spine_name::NameError::Revoked(raw.to_string()).to_string(),
                }
            }
            Ok(Some(record)) => {
                let _ = state.names.cache_record(record.clone());
                match serde_json::to_value(&record) {
                    Ok(value) => spine_protocol::NameResolution::Resolved {
                        record: value,
                        provenance: spine_protocol::NameProvenance::Network,
                    },
                    Err(e) => spine_protocol::NameResolution::Invalid {
                        name: raw.to_string(),
                        reason: e.to_string(),
                    },
                }
            }
            Ok(None) => {
                // Remember the miss: an agent retrying a name nobody has should
                // not re-walk the keyspace every time.
                state.names.cache_negative(&uri);
                spine_protocol::NameResolution::NotFound {
                    name: raw.to_string(),
                }
            }
            Err(reason) => spine_protocol::NameResolution::Unavailable {
                name: raw.to_string(),
                reason,
            },
        },
        Err(e) => spine_protocol::NameResolution::Invalid {
            name: raw.to_string(),
            reason: e.to_string(),
        },
    }
}

/// Walk the mesh for a name, if this node has joined one.
async fn resolve_over_mesh(
    state: &Arc<BrowserState>,
    uri: &spine_name::SpineUri,
) -> Result<Option<spine_name::NameRecord>, String> {
    let Some(mesh) = state.mesh_names.as_ref() else {
        return Ok(None);
    };
    if mesh.peer_count().await == 0 {
        return Err("namespace has no reachable peers; only local names are available".into());
    }
    match mesh.resolve_with_log(uri).await {
        Ok((record, log)) => {
            // The local resolver needs the log to admit, and so cache, a
            // rotated name's record. A refusal only costs the cache entry.
            if let Some(log) = log {
                if let Err(error) = state.names.offer_log(log) {
                    debug!(%error, "succession log from the mesh was not kept");
                }
            }
            Ok(Some(record))
        }
        Err(spine_mesh::naming_mesh::NameMeshError::NotFound(_)) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// The `ok` value of a command's reply. Most commands answer with a JSON-shaped
/// value; `read_resource` and `capability_query` answer with their typed
/// results so that byte fields reach the wire as CBOR byte strings.
enum Output {
    Json(serde_json::Value),
    Chunk(spine_protocol::resource::ResourceChunk),
    Capabilities(spine_protocol::CapabilityAdvertisement),
}

impl serde::Serialize for Output {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Output::Json(value) => value.serialize(serializer),
            Output::Chunk(chunk) => chunk.serialize(serializer),
            Output::Capabilities(ad) => ad.serialize(serializer),
        }
    }
}

/// A command's answer: the `ok` value, or the `err` to send. The frame
/// header's request id correlates it; nothing here carries an id.
type Answer = Result<Output, WireError>;

/// Programs compiling and running at once, process-wide. Each runs on a
/// blocking thread and can use a core for a while, so the pool is small.
const EXECUTE_CONCURRENCY: usize = 4;

/// How long `execute_binary` waits for an executor permit before it
/// answers `overloaded`.
const EXECUTE_PERMIT_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Reactive state variables one session may hold.
const MAX_SESSION_STATE_ENTRIES: usize = 256;

/// Knowledge entries one session may hold.
const MAX_KNOWLEDGE_ENTRIES_PER_SESSION: usize = 1024;

/// Navigations (fetches) one `execute_binary` may request.
const MAX_NAVIGATIONS_PER_EXECUTE: usize = 4;

/// An `err` with a code from `spine_protocol::body`.
fn fail(code: &str, message: String) -> WireError {
    WireError::new(code, message)
}

/// The `err` for a failed navigation. The fetcher's error carries its wire
/// code (`outbound_address_denied`, ...), which passes through unchanged; an
/// HTTP status goes in `details.status`.
fn navigation_failure(error: &outbound::FetchError) -> WireError {
    let message = format!("the outbound request was refused or failed ({error})");
    let wire = fail(error.code(), message);
    match error.status() {
        Some(status) => wire.with_details(serde_json::json!({ "status": status })),
        None => wire,
    }
}

/// The `err` for a failed `read_resource`, by the error's variant.
fn read_failure(error: &spine_core::resources::ReadError) -> WireError {
    use spine_core::resources::ReadError;
    let code = match error {
        ReadError::NotFound => ERR_RESOURCE_NOT_FOUND,
        ReadError::InvalidRange(_) => ERR_INVALID_REQUEST,
        ReadError::Internal(_) => ERR_INTERNAL,
    };
    fail(code, error.to_string())
}

#[instrument(skip_all)]
async fn handle_command(
    state: &Arc<BrowserState>,
    session_id: &str,
    command: BrowserCommand,
    request_id: u64,
) -> Answer {
    let timer = COMMAND_LATENCY.start_timer();
    COMMANDS_TOTAL.inc();
    // The frame id names the request in traces; only the research build's
    // cluster search forwards it.
    #[cfg(not(feature = "research"))]
    let _ = request_id;

    let response = match command {
        BrowserCommand::NamespaceStatus => {
            let result = match &state.mesh_names {
                Some(mesh) => {
                    let key: String = mesh
                        .local_key()
                        .as_bytes()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    let peers=mesh.peers().await.into_iter().map(|p| {
                        let key:String=p.info.id.as_bytes().iter().map(|b|format!("{b:02x}")).collect();
                        serde_json::json!({"agent_id":p.agent_id,"public_key":key,"endpoints":p.info.endpoints})
                    }).collect::<Vec<_>>();
                    serde_json::json!({"enabled":true,"agent_id":mesh.agent_id(),"public_key":key,"peers":peers,
                        "connectivity":if peers.is_empty(){"local_only"}else{"connected"},"tasks_enabled":state.config.namespace.tasks_enabled})
                }
                None => serde_json::json!({"enabled":false,"connectivity":"local_only","peers":[]}),
            };
            Ok(Output::Json(result))
        }
        BrowserCommand::MeshCall {
            peer_id,
            peer_key,
            payload,
        } => match (&state.mesh_names, uuid::Uuid::parse_str(&peer_id)) {
            (Some(mesh), Ok(id)) => mesh
                .application_call(spine_mesh::AgentId(id), peer_key, payload)
                .await
                .map(Output::Json)
                .map_err(|e| fail(ERR_MESH_CALL_FAILED, e.to_string())),
            (None, _) => Err(fail(
                ERR_NAMESPACE_UNAVAILABLE,
                "namespace is unavailable".into(),
            )),
            (_, Err(_)) => Err(fail(ERR_INVALID_PEER_ID, "peer id must be a UUID".into())),
        },
        BrowserCommand::ReadResource {
            hash,
            offset,
            max_bytes,
        } => match state.resources.read(hash, offset, max_bytes) {
            Ok(chunk) => Ok(Output::Chunk(chunk)),
            Err(e) => Err(read_failure(&e)),
        },
        BrowserCommand::PublishResource {
            record,
            content_type,
            data,
        } => {
            type Published = (spine_name::NameRecord, Option<spine_name::SuccessionLog>);
            let result = (|| -> Result<Published, WireError> {
                // As in `PublishName`: the succession log may ride beside the
                // record's fields.
                let log = spine_name::publication_log(&record)
                    .transpose()
                    .map_err(|e| {
                        fail(
                            ERR_MALFORMED_RECORD,
                            format!("malformed succession log: {e}"),
                        )
                    })?;
                let record: spine_name::NameRecord = serde_json::from_value(record)
                    .map_err(|e| fail(ERR_MALFORMED_RECORD, e.to_string()))?;
                if record.is_expired(unix_now()) {
                    return Err(fail(ERR_RECORD_EXPIRED, "record is expired".into()));
                }
                // The log is offered before the resource store judges the
                // record, which it does against the resolver's book. A valid
                // log is kept even if the publication then fails.
                if let Some(log) = &log {
                    if spine_name::log_inception_key(log)
                        .is_some_and(|key| record.name.public_key() != Some(&key))
                    {
                        return Err(fail(
                            ERR_RECORD_REJECTED,
                            "succession log is for another name".into(),
                        ));
                    }
                    state
                        .names
                        .offer_log(log.clone())
                        .map_err(|e| fail(ERR_RECORD_REJECTED, e.to_string()))?;
                }
                let (hash, created) = state
                    .resources
                    .put_as(
                        spine_core::resources::publisher_of(&record),
                        &record,
                        &content_type,
                        &data,
                    )
                    .map_err(|e| fail(ERR_PUBLISH_FAILED, e.to_string()))?;
                if let Err(e) = state.names.publish(record.clone()) {
                    // Drop the blob only if this request added it; an earlier
                    // publication of the same content still refers to it.
                    if created {
                        let _ = state.resources.remove(hash);
                    }
                    return Err(fail(ERR_RECORD_REJECTED, e.to_string()));
                }
                Ok((record, log))
            })();
            match result {
                Ok((record, log)) => {
                    let publication = publication_report(state, record, log).await;
                    Ok(Output::Json(publication))
                }
                Err(e) => Err(e),
            }
        }
        BrowserCommand::ResolveName {
            name,
            if_none_match,
        } => {
            let resolution = resolve_one(state, &name, if_none_match).await;
            Ok(Output::Json(
                serde_json::to_value(resolution).unwrap_or_default(),
            ))
        }

        BrowserCommand::ResolveNames { names } => {
            // One round trip for the whole batch. Each name resolves
            // independently, so a single bad name never fails the others.
            if names.len() > 256 {
                return Err(fail(
                    ERR_BATCH_TOO_LARGE,
                    "resolution batch exceeds 256 names".into(),
                ));
            }
            let mut results = Vec::with_capacity(names.len());
            for batch in names.chunks(16) {
                let mut tasks = Vec::new();
                for name in batch {
                    let state = state.clone();
                    let name = name.clone();
                    tasks.push(tokio::spawn(async move {
                        resolve_one(&state, &name, None).await
                    }));
                }
                // Await handles in input order; all sixteen tasks run concurrently.
                for task in tasks {
                    match task.await {
                        Ok(result) => results.push(result),
                        Err(_) => return Err(fail(ERR_INTERNAL, "resolution task failed".into())),
                    }
                }
            }
            Ok(Output::Json(serde_json::json!({
                "resolutions": serde_json::to_value(&results).unwrap_or_default(),
            })))
        }

        BrowserCommand::FindProviders { capability, limit } => {
            use spine_name::Resolver;
            match state.names.find_providers(&capability).await {
                Ok(mut providers) => {
                    // An empty local index is not an answer — "who can do X" is
                    // exactly the question the mesh exists to route.
                    if let Some(mesh) = state.mesh_names.as_ref() {
                        if mesh.peer_count().await > 0 {
                            match mesh.find_providers(&capability).await {
                                Ok(remote) => {
                                    for record in remote {
                                        if !record.is_expired(unix_now())
                                            && state.names.cache_record(record.clone()).is_ok()
                                        {
                                            match providers
                                                .iter()
                                                .position(|p| p.name == record.name)
                                            {
                                                Some(i) if record.supersedes(&providers[i]) => {
                                                    providers[i] = record
                                                }
                                                Some(_) => {}
                                                None => providers.push(record),
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    return Err(fail(
                                        ERR_LOOKUP_UNAVAILABLE,
                                        format!("capability lookup unavailable: {e}"),
                                    ))
                                }
                            }
                        }
                    }
                    providers.sort_by(|a, b| {
                        b.endpoints
                            .len()
                            .cmp(&a.endpoints.len())
                            .then(b.published_at.cmp(&a.published_at))
                            .then(a.name.to_string().cmp(&b.name.to_string()))
                    });
                    if let Some(n) = limit {
                        providers.truncate(n);
                    }
                    Ok(Output::Json(serde_json::json!({
                        "capability": capability,
                        "providers": serde_json::to_value(&providers).unwrap_or_default(),
                        "count": providers.len(),
                    })))
                }
                Err(e) => Err(fail(ERR_LOOKUP_FAILED, e.to_string())),
            }
        }

        BrowserCommand::PublishName { record } => {
            // `spine name publish --log` sends the name's succession log beside
            // the record's fields; without it a rotated name's record is refused.
            let log = match spine_name::publication_log(&record) {
                Some(Ok(log)) => Some(log),
                Some(Err(e)) => {
                    return Err(fail(
                        ERR_MALFORMED_RECORD,
                        format!("malformed succession log: {e}"),
                    ))
                }
                None => None,
            };
            match serde_json::from_value::<spine_name::NameRecord>(record) {
                // publish() verifies the signature, so an unsigned or forged
                // record is rejected here rather than becoming servable state.
                Ok(record) => {
                    if record.is_expired(unix_now()) {
                        return Err(fail(ERR_RECORD_EXPIRED, "record is expired".into()));
                    }
                    let published = match log.clone() {
                        Some(log) => state.names.publish_with_log(record.clone(), log),
                        None => state.names.publish(record.clone()),
                    };
                    match published {
                        Ok(()) => {
                            let publication = publication_report(state, record, log).await;
                            Ok(Output::Json(publication))
                        }
                        Err(e) => Err(fail(ERR_RECORD_REJECTED, format!("record rejected: {e}"))),
                    }
                }
                Err(e) => Err(fail(ERR_MALFORMED_RECORD, format!("malformed record: {e}"))),
            }
        }

        BrowserCommand::FetchName {
            name,
            if_none_match,
        } => {
            // Resolve first, then hand back the endpoints to dial. Fetching the
            // bytes is the transport layer's job; conflating the two here would
            // hide which half failed.
            let resolution = resolve_one(state, &name, if_none_match).await;
            let endpoints = match &resolution {
                spine_protocol::NameResolution::Resolved { record, .. } => {
                    serde_json::from_value::<spine_name::NameRecord>(record.clone())
                        .map(|r| {
                            r.endpoints_by_priority()
                                .into_iter()
                                .map(|e| format!("{}://{}", e.transport, e.address))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                }
                _ => Vec::new(),
            };
            Ok(Output::Json(serde_json::json!({
                "resolution": serde_json::to_value(&resolution).unwrap_or_default(),
                "endpoints": endpoints,
            })))
        }

        BrowserCommand::CrawlNames {
            seed,
            max_depth,
            max_visits,
        } => match spine_name::SpineUri::parse(&seed) {
            Ok(seed_uri) => {
                use spine_name::Resolver;
                let mut budget = spine_name::CrawlBudget::default();
                if let Some(d) = max_depth {
                    budget = budget.with_max_depth(d);
                }
                if let Some(v) = max_visits {
                    budget = budget.with_max_visits(v);
                }
                let mut frontier = spine_name::CrawlFrontier::new(budget);
                frontier.seed(seed_uri);

                let mut visited = Vec::new();
                while let Some(visit) = frontier.next_visit() {
                    match state.names.resolve(&visit.uri).await {
                        Ok(res) => {
                            frontier.expand(&visit.uri, &res.record.links, visit.depth);
                            visited.push(serde_json::json!({
                                "name": visit.uri.to_string(),
                                "depth": visit.depth,
                                "capabilities": res.record.capabilities,
                                "links": res.record.links.len(),
                            }));
                        }
                        Err(_) => visited.push(serde_json::json!({
                            "name": visit.uri.to_string(),
                            "depth": visit.depth,
                            "unresolved": true,
                        })),
                    }
                }

                Ok(Output::Json(serde_json::json!({
                    "visited": visited,
                    // Reporting what was *not* walked keeps a bounded crawl
                    // from reading as an exhaustive one.
                    "skipped": frontier.skipped().len(),
                    "pending": frontier.pending(),
                })))
            }
            Err(e) => Err(fail(ERR_INVALID_SEED, format!("invalid seed name: {e}"))),
        },

        BrowserCommand::Navigate { url } => {
            match state.fetcher.fetch_text_for(session_id, &url).await {
                Ok(html) => {
                    // Transpile HTML to HLB for agentic interaction
                    #[cfg(feature = "research")]
                    let hlb = spine_human::HumanTranspiler::transpile(&html, "", "")
                        .unwrap_or_else(|e| {
                            warn!(error_kind = %std::any::type_name_of_val(&e), "Page transpilation failed");
                            spine_protocol::SpineBinary {
                                instructions: vec![],
                                data: vec![],
                                render_start: 0,
                                exported_functions: std::collections::BTreeMap::new(),
                                capabilities: vec![],
                            }
                        });
                    // Without the research feature there is no transpiler: the
                    // session gets the same empty binary a failed transpile yields.
                    #[cfg(not(feature = "research"))]
                    let hlb = spine_protocol::SpineBinary {
                        instructions: vec![],
                        data: vec![],
                        render_start: 0,
                        exported_functions: std::collections::BTreeMap::new(),
                        capabilities: vec![],
                    };

                    if let Some(mut session) = state.sessions.get_mut(session_id) {
                        session.current_url = Some(url);
                        session.current_html = Some(html);
                        session.current_binary = Some(hlb);
                        session.current_vdom = None;
                        session.cached_ur = None; // Invalidate cached UR on navigation
                    } else {
                        return Err(fail(ERR_SESSION_EXPIRED, "Session expired".into()));
                    }
                    Ok(Output::Json(
                        serde_json::json!({"status": "success", "session_id": session_id}),
                    ))
                }
                Err(e) => Err(navigation_failure(&e)),
            }
        }
        BrowserCommand::GetUR => {
            if let Some(mut session) = state.sessions.get_mut(session_id) {
                // Populate cache on first access (avoids re-parsing on repeated GetUR calls)
                if session.cached_ur.is_none() {
                    if let Some(html) = session.current_html.clone() {
                        match spine_parser::parse_html(&html) {
                            Ok(ur) => session.cached_ur = Some(ur),
                            Err(e) => {
                                return Err(fail(
                                    ERR_PARSE_ERROR,
                                    format!("the loaded page does not parse: {e}"),
                                ))
                            }
                        }
                    }
                }
                if let Some(ur) = &session.cached_ur {
                    let vdom_tree = session.current_vdom.as_ref().map(|v| v.to_tree());
                    let vdom_ur = session.current_vdom.as_ref().map(|v| v.to_ur());
                    Ok(Output::Json(serde_json::json!({
                        "parsed_ur": serde_json::to_value(ur).unwrap_or_default(),
                        "vdom_tree": vdom_tree,
                        "vdom_ur": vdom_ur,
                    })))
                } else {
                    Err(fail(ERR_NO_PAGE, "No page loaded in session".to_string()))
                }
            } else {
                Err(fail(ERR_SESSION_NOT_FOUND, "Session not found".to_string()))
            }
        }
        BrowserCommand::GetRawHTML => {
            if let Some(session) = state.sessions.get(session_id) {
                if let Some(html) = &session.current_html {
                    Ok(Output::Json(serde_json::json!({ "html": html })))
                } else {
                    Err(fail(ERR_NO_PAGE, "No page loaded".to_string()))
                }
            } else {
                Err(fail(ERR_SESSION_NOT_FOUND, "Session not found".to_string()))
            }
        }
        BrowserCommand::ExecuteBinary(bin) => {
            info!(
                "Executing HLB with {} instructions and {} capabilities",
                bin.instructions.len(),
                bin.capabilities.len()
            );

            // Capability check
            for cap in &bin.capabilities {
                match cap.as_str() {
                    "network" | "storage" | "memory" | "search" => {
                        // Allowed by default for now
                    }
                    _ => {
                        return Err(fail(
                            ERR_CAPABILITY_DENIED,
                            format!("Unauthorized or unknown capability: {}", cap),
                        ));
                    }
                }
            }

            // Compiling and running is CPU work that cannot be cancelled
            // once started, so it runs on a blocking thread behind a
            // process-wide permit, and a saturated executor answers
            // `overloaded` instead of queueing. The bound on how long a
            // run takes is the fuel plus the host budgets inside
            // `spine_wasm` (see its `MAX_*` constants), not a timeout.
            let permit = match tokio::time::timeout(
                EXECUTE_PERMIT_WAIT,
                state.execute_permits.clone().acquire_owned(),
            )
            .await
            {
                Ok(Ok(permit)) => permit,
                _ => {
                    return Err(fail(
                        ERR_OVERLOADED,
                        "the executor is busy; retry later".to_string(),
                    ));
                }
            };
            let runtime = state.wasm_runtime.clone();
            let program = bin.clone();
            let ran = tokio::task::spawn_blocking(move || {
                // The permit is held until the work ends, even if the
                // client has gone.
                let _permit = permit;
                runtime.execute(&program)
            })
            .await;

            // A program that fails to compile, instantiate or run is the
            // client's error, not the daemon's: answer it and keep serving.
            let result = match ran {
                Ok(Ok(result)) => result,
                Ok(Err(e)) => {
                    return Err(fail(ERR_EXECUTION_FAILED, format!("execution failed: {e}")));
                }
                Err(_) => {
                    return Err(fail(
                        ERR_EXECUTION_FAILED,
                        "execution failed: the executor stopped".to_string(),
                    ));
                }
            };

            // Everything the program asked for is checked here, before
            // any of it is carried out: a refusal leaves no partial effect.
            // The declared capabilities are what the action needs, not a
            // list the daemon grants.
            let declares = |cap: &str| bin.capabilities.iter().any(|c| c == cap);
            let mut navigations = 0usize;
            let mut stores = 0usize;
            for action in &result.actions {
                let needs = match action {
                    spine_wasm::WasmAction::Navigate(_) => {
                        navigations += 1;
                        Some("network")
                    }
                    spine_wasm::WasmAction::Search(_) => Some("search"),
                    spine_wasm::WasmAction::StoreKnowledge { .. } => {
                        stores += 1;
                        Some("storage")
                    }
                    spine_wasm::WasmAction::QueryKnowledge { .. }
                    | spine_wasm::WasmAction::Reason(_) => None,
                };
                if let Some(cap) = needs {
                    if !declares(cap) {
                        return Err(fail(
                            ERR_CAPABILITY_DENIED,
                            format!("the program requested an action that needs the \"{cap}\" capability, which it did not declare"),
                        ));
                    }
                }
            }
            if navigations > MAX_NAVIGATIONS_PER_EXECUTE {
                return Err(fail(
                    ERR_EXECUTION_FAILED,
                    format!(
                        "execution failed: {navigations} navigations requested, at most {MAX_NAVIGATIONS_PER_EXECUTE} allowed"
                    ),
                ));
            }
            if let Some(session) = state.sessions.get(session_id) {
                let new_names: std::collections::HashSet<&str> = result
                    .events
                    .iter()
                    .filter(|e| matches!(e.name.as_str(), "state_declared" | "state_updated"))
                    .filter_map(|e| e.payload.get("name").and_then(|v| v.as_str()))
                    .filter(|n| !session.state.contains_key(*n))
                    .collect();
                if session.state.len() + new_names.len() > MAX_SESSION_STATE_ENTRIES {
                    return Err(fail(
                        ERR_EXECUTION_FAILED,
                        format!(
                            "execution failed: a session may hold at most {MAX_SESSION_STATE_ENTRIES} state variables"
                        ),
                    ));
                }
            }
            let held = state
                .knowledge_base
                .get(session_id)
                .map_or(0, |entries| entries.len());
            if held + stores > MAX_KNOWLEDGE_ENTRIES_PER_SESSION {
                return Err(fail(
                    ERR_EXECUTION_FAILED,
                    format!(
                        "execution failed: a session may hold at most {MAX_KNOWLEDGE_ENTRIES_PER_SESSION} knowledge entries"
                    ),
                ));
            }

            // Store binary in session for reactivity
            if let Some(mut session) = state.sessions.get_mut(session_id) {
                session.current_binary = Some(bin.clone());
            }

            // Update session VDOM and compute patches
            let new_vdom = VirtualDom::from_wasm(&result);
            let mut patches = Vec::new();

            if let Some(mut session) = state.sessions.get_mut(session_id) {
                if let Some(old_vdom) = &session.current_vdom {
                    patches = new_vdom.diff(old_vdom);
                }
                session.current_vdom = Some(new_vdom);

                // Handle state events from WASM
                for event in &result.events {
                    match event.name.as_str() {
                        "state_declared" | "state_updated" => {
                            if let (Some(name), Some(value)) = (
                                event.payload.get("name").and_then(|v| v.as_str()),
                                event.payload.get("value"),
                            ) {
                                session.state.insert(name.to_string(), value.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Handle agentic actions from WASM
            for action in &result.actions {
                match action {
                    spine_wasm::WasmAction::Navigate(url) => {
                        info!("WASM requested navigation");
                        let state_clone = state.clone();
                        let url_clone = url.clone();
                        let sid_clone = session_id.to_string();
                        tokio::spawn(async move {
                            if let Ok(html) = state_clone
                                .fetcher
                                .fetch_text_for(&sid_clone, &url_clone)
                                .await
                            {
                                if let Some(mut session) = state_clone.sessions.get_mut(&sid_clone)
                                {
                                    session.current_url = Some(url_clone);
                                    session.current_html = Some(html);
                                }
                            }
                        });
                    }
                    spine_wasm::WasmAction::Search(query) => {
                        info!("WASM requested search");
                        #[cfg(feature = "research")]
                        {
                            let cluster = state.cluster.lock().await;
                            let _ = cluster.broadcast_search(query.clone(), request_id.to_string());
                        }
                        #[cfg(not(feature = "research"))]
                        let _ = query;
                    }
                    spine_wasm::WasmAction::StoreKnowledge { key, value, tags } => {
                        info!("WASM requested knowledge storage");
                        if let Some(_session) = state.sessions.get_mut(session_id) {
                            // Store in local KB, under the session's own id
                            // exactly as the `store_knowledge` command does.
                            // The program's key stays in the entry; it does
                            // not choose which entry list is written.
                            let entry = KnowledgeEntry {
                                key: key.clone(),
                                value: value.clone(),
                                tags: tags.clone(),
                                timestamp_ns: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_nanos()
                                    as u64,
                            };
                            state
                                .knowledge_base
                                .entry(session_id.to_string())
                                .or_default()
                                .push(entry);

                            #[cfg(feature = "research")]
                            {
                                let cluster = state.cluster.lock().await;
                                let _ = cluster.broadcast_knowledge(
                                    key.clone(),
                                    value.clone(),
                                    tags.clone(),
                                );
                            }
                        }
                    }
                    spine_wasm::WasmAction::QueryKnowledge {
                        query: _,
                        tags: _,
                        limit: _,
                    } => {
                        info!("WASM requested knowledge query");
                    }
                    spine_wasm::WasmAction::Reason(query) => {
                        info!("WASM requested reasoning");
                        let _ = query;
                    }
                }
            }

            Ok(Output::Json(serde_json::json!({
                "status": "executed",
                "stats": {
                    "instructions_executed": result.stats.instructions_executed,
                    "compile_time_us": result.stats.compile_time_us,
                    "execution_time_us": result.stats.execution_time_us,
                    "wasm_size_bytes": result.stats.wasm_size_bytes,
                    "memory_used_bytes": result.stats.memory_used_bytes,
                },
                "elements": result.elements,
                "events": result.events,
                "actions": result.actions,
                "latent_streams_count": result.latent_streams.len(),
                "patches": patches,
            })))
        }
        BrowserCommand::StoreKnowledge { key, value, tags } => {
            let entry = KnowledgeEntry {
                key: key.clone(),
                value: value.clone(),
                tags: tags.clone(),
                timestamp_ns: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
            };
            state
                .knowledge_base
                .entry(session_id.to_string())
                .or_default()
                .push(entry);

            // Broadcast to cluster
            #[cfg(feature = "research")]
            {
                let cluster = state.cluster.lock().await;
                let _ = cluster.broadcast_knowledge(key, value, tags);
            }
            #[cfg(not(feature = "research"))]
            let _ = (key, value, tags);

            Ok(Output::Json(
                serde_json::json!({ "status": "knowledge_stored" }),
            ))
        }
        BrowserCommand::QueryKnowledge { query, tags, limit } => {
            let entries = state
                .knowledge_base
                .get(session_id)
                .map(|e| e.clone())
                .unwrap_or_default();
            let results: Vec<_> = entries
                .into_iter()
                .filter(|e| {
                    let matches_tags = tags.is_empty() || tags.iter().all(|t| e.tags.contains(t));
                    let matches_query = query.is_empty() || e.key.contains(&query);
                    matches_tags && matches_query
                })
                .take(limit)
                .collect();
            Ok(Output::Json(serde_json::json!({ "results": results })))
        }
        BrowserCommand::GetBinaryCapabilities => {
            // What the current binary declared; a declaration the daemon
            // checks actions against, not a grant.
            let capabilities = state
                .sessions
                .get(session_id)
                .and_then(|s| s.current_binary.as_ref().map(|b| b.capabilities.clone()))
                .unwrap_or_default();
            Ok(Output::Json(
                serde_json::json!({ "capabilities": capabilities }),
            ))
        }
        // `handle_session` answers these two before a session is involved.
        BrowserCommand::ToolCall(_) | BrowserCommand::CapabilityQuery(_) => Err(fail(
            ERR_INTERNAL,
            "this command is not served by a session".into(),
        )),
    };

    timer.observe_duration();
    response
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = SpineConfig::try_load()?;
    init_telemetry("spine-core")?;
    info!("Starting SPINE Agentic Browser...");
    info!(
        "Config: host={}, port={}, max_sessions={}, tls={}",
        config.server.host, config.server.port, config.server.max_sessions, config.tls.enabled
    );

    let port = config.server.port;
    #[cfg(feature = "research")]
    let cluster_port = port + config.cluster.port_offset;
    #[cfg(feature = "research")]
    let cluster_addr: std::net::SocketAddr = format!("{}:{}", config.server.host, cluster_port)
        .parse()
        .context("invalid cluster address")?;

    #[cfg(feature = "research")]
    let capabilities = NodeCapabilities {
        supports_wasm: true,
        supports_chameleon: false,
        supports_speculation: false,
        max_sessions: config.server.max_sessions,
        region: Some(config.cluster.region.clone()),
        skills: config.cluster.skills.clone(),
    };

    #[cfg(feature = "research")]
    let mut cluster_node = ClusterNode::new(cluster_addr, capabilities);
    #[cfg(feature = "research")]
    cluster_node.start().await?;

    // Join the namespace before serving, so the first client request already has
    // a mesh to route through. A node that cannot reach a seed still starts: it
    // serves the names it holds and says plainly that it is not routing.
    let name_backend: Arc<dyn spine_storage::StorageBackend> =
        if config.namespace.records_path.is_empty() {
            Arc::new(spine_storage::InMemoryBackend::new())
        } else {
            Arc::new(spine_storage::sqlite::SqliteBackend::open(
                &config.namespace.records_path,
            )?)
        };
    let application = namespace::task_application(&config.namespace, name_backend.clone())?;
    let mesh_names = if config.namespace.enabled {
        match namespace::join_with_application(&config.namespace, &config.server.host, application)
            .await
        {
            Ok(resolver) => Some(resolver),
            Err(e) => {
                error!("failed to join the spine:// namespace: {e:#}");
                None
            }
        }
    } else {
        None
    };

    let names = Arc::new(spine_name::LocalResolver::persistent(
        unix_now,
        name_backend.clone(),
    )?);
    if let Some(mesh) = &mesh_names {
        for record in names.published_records()? {
            if !record.is_expired(unix_now()) {
                // The mesh store keeps its own log book, in memory: give it the
                // persisted log first, or a rotated name's record is refused.
                let log = record
                    .name
                    .public_key()
                    .and_then(|key| names.succession_log(key));
                if let Some(log) = log {
                    mesh.offer_log(log).await?;
                }
                mesh.publish_local(record).await?;
            }
        }
    }
    namespace::spawn_owner_renewal(names.clone(), mesh_names.clone(), &config.namespace)?;
    let state = Arc::new(BrowserState {
        // A blob named by a record this node currently publishes is never
        // evicted, including after the record is renewed.
        resources: spine_core::resources::ResourceStore::new(name_backend)
            .with_reference_source({
                let names = names.clone();
                move || {
                    let now = unix_now();
                    let records = names
                        .published_records()
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                    Ok(records
                        .into_iter()
                        .filter(|record| !record.is_expired(now))
                        .filter_map(|record| record.content_hash)
                        .collect())
                }
            })
            // A resource's binding record is judged against the succession logs
            // the resolver holds: the current key of a rotated name, never a
            // superseded one (spec 02, section 10).
            .with_record_authority({
                let names = names.clone();
                move |record: &spine_name::NameRecord| {
                    names.authorise(record).map_err(anyhow::Error::from)
                }
            }),
        sessions: DashMap::new(),
        session_gate: Default::default(),
        snapshot_gate: Mutex::new(()),
        knowledge_base: DashMap::new(),
        #[cfg(feature = "research")]
        proposals: DashMap::new(),
        #[cfg(feature = "research")]
        plans: DashMap::new(),
        fetcher: outbound::Fetcher::new(config.outbound.clone())?,
        #[cfg(feature = "research")]
        cluster: Mutex::new(cluster_node),
        rate_limits: DashMap::new(),
        wasm_runtime: Arc::new(WasmRuntime::new().expect("Failed to initialize WASM runtime")),
        execute_permits: Arc::new(tokio::sync::Semaphore::new(EXECUTE_CONCURRENCY)),
        connections: spine_core::connections::ConnectionBudget::new(
            config.server.max_connections,
            config.server.max_connections_per_ip,
        ),
        start_time: std::time::Instant::now(),
        shutting_down: std::sync::atomic::AtomicBool::new(false),
        shutdown: tokio::sync::watch::channel(false).0,
        config: config.clone(),
        names,
        mesh_names,
    });

    state.load_sessions().context("load retained sessions")?;
    info!("Loaded {} persisted sessions", state.sessions.len());

    // Start session persistence task
    let persistence_state = state.clone();
    let persist_secs = config.server.persistence_interval_secs;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(persist_secs));
        loop {
            interval.tick().await;
            if let Err(e) = persistence_state.save_sessions().await {
                error!("Failed to save sessions: {}", e);
            } else {
                debug!("Sessions persisted to disk");
            }
        }
    });

    // Start metrics & health server
    let metrics_state = state.clone();
    let namespace_metrics_state = state.clone();
    let health_state = state.clone();
    let ready_state = state.clone();
    let metrics_app = Router::new()
        .route("/metrics", get(move || {
            let state = namespace_metrics_state.clone();
            async move {
            SESSIONS_ACTIVE.set(state.sessions.len() as i64);
            let encoder = TextEncoder::new();
            let metric_families = prometheus::gather();
            let mut buffer = vec![];
            let _ = encoder.encode(&metric_families, &mut buffer);
            let mut body = String::from_utf8(buffer).unwrap_or_default();
            spine_core::operations::append_metrics(&mut body, state.connections.snapshot(), state.fetcher.snapshot());
            if let Some(mesh) = &state.mesh_names {
                let metrics = mesh.metrics().await;
                for (name, value) in [
                    ("lookups_started", metrics.lookups_started),
                    ("lookups_resolved", metrics.lookups_resolved),
                    ("lookups_timed_out", metrics.lookups_timed_out),
                    ("announcements_rejected", metrics.announcements_rejected),
                    ("replicas_sent", metrics.replicas_sent),
                    ("replicas_failed", metrics.replicas_failed),
                    ("records_expired", metrics.records_expired),
                ] {
                    body.push_str(&format!("# TYPE spine_namespace_{name}_total counter\nspine_namespace_{name}_total {value}\n"));
                }
                body.push_str(&format!("# TYPE spine_mesh_pending_calls gauge\nspine_mesh_pending_calls {}\n", mesh.pending_application_calls()));
            }
            body
            }
        }))
        .route("/health", get(move || {
            let state = health_state.clone();
            async move {
                let uptime = state.start_time.elapsed().as_secs();
                let connections = state.connections.snapshot();
                let active = connections.active;
                let sessions = state.sessions.len();
                let shutting_down = state.shutting_down.load(std::sync::atomic::Ordering::Relaxed);

                let status = if shutting_down { "draining" } else { "healthy" };
                let code = if shutting_down { 503u16 } else { 200 };

                let body = serde_json::json!({
                    "status": status,
                    "uptime_secs": uptime,
                    "active_connections": active,
                    "sessions": sessions,
                    "connections": connections,
                    "outbound": state.fetcher.snapshot(),
                    "version": env!("CARGO_PKG_VERSION"),
                });

                (
                    axum::http::StatusCode::from_u16(code).unwrap_or(axum::http::StatusCode::OK),
                    axum::response::Json(body),
                )
            }
        }))
        .route("/ready", get(move || {
            let state = ready_state.clone();
            async move {
                let shutting_down = state.shutting_down.load(std::sync::atomic::Ordering::Relaxed);
                let sessions = state.sessions.len();
                let max = state.config.server.max_sessions;
                let readiness = spine_core::operations::readiness(shutting_down, sessions, max, state.connections.snapshot());
                let ready = readiness.ready;
                let mut body = serde_json::json!(readiness);
                body["namespace"] = serde_json::json!(match &state.mesh_names {
                    Some(mesh) if !mesh.peers().await.is_empty() => "peers_known",
                    Some(_) => "local_only",
                    None => "disabled",
                });
                body["tasks_enabled"] = serde_json::json!(state.config.namespace.tasks_enabled);
                body["pending_mesh_calls"] = serde_json::json!(state.mesh_names.as_ref().map_or(0, |mesh| mesh.pending_application_calls()));
                body["outbound"] = serde_json::json!(state.fetcher.snapshot());

                let code = if ready { 200u16 } else { 503 };
                (
                    axum::http::StatusCode::from_u16(code).unwrap_or(axum::http::StatusCode::OK),
                    axum::response::Json(body),
                )
            }
        }))
        .route("/dashboard", get(move || {
            let state = metrics_state.clone();
            async move {
                let mut html = String::from("<html><head><title>SPINE Dashboard</title>");
                html.push_str("<style>body{font-family:sans-serif;padding:20px;} table{border-collapse:collapse;width:100%;} th,td{border:1px solid #ddd;padding:8px;text-align:left;} th{background-color:#f2f2f2;}</style>");
                html.push_str("</head><body><h1>SPINE Session Monitor</h1>");
                html.push_str(&format!("<p>Active Sessions: {}</p>", state.sessions.len()));
                html.push_str("<table><tr><th>Session ID</th></tr>");
                for entry in state.sessions.iter() {
                    let id = entry.key();
                    let safe_id=id.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;");
                    html.push_str(&format!("<tr><td>{}</td></tr>",safe_id));
                }
                html.push_str("</table></body></html>");
                axum::response::Html(html)
            }
        }));

    let metrics_port = config.server.metrics_port;
    let metrics_host = config.server.host.clone();
    tokio::spawn(async move {
        let addr: std::net::SocketAddr = match format!("{}:{}", metrics_host, metrics_port).parse()
        {
            Ok(a) => a,
            Err(e) => {
                error!("Invalid metrics address: {}", e);
                return;
            }
        };
        info!("Health & metrics server on http://{}", addr);
        // axum 0.7 dropped `Server` in favour of binding the listener yourself
        // and handing it to `axum::serve`, so bind failures are now reported
        // separately from serve failures instead of being folded together.
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                error!("Metrics server bind error: {}", e);
                return;
            }
        };
        if let Err(e) = axum::serve(listener, metrics_app).await {
            error!("Metrics server error: {}", e);
        }
    });

    // Start cluster event listener
    #[cfg(feature = "research")]
    let cluster_state = state.clone();
    #[cfg(feature = "research")]
    let event_rx = {
        let cluster = state.cluster.lock().await;
        cluster.get_event_receiver()
    };

    #[cfg(feature = "research")]
    tokio::spawn(async move {
        let mut rx = event_rx.lock().await;
        while let Some(event) = rx.recv().await {
            match event {
                spine_cluster::ClusterEvent::SessionTransferRequested {
                    session_id,
                    from_node,
                } => {
                    info!(
                        "Session transfer requested for {} from node {}",
                        session_id, from_node
                    );
                    // In a real implementation, we might need to acknowledge or prepare
                }
                spine_cluster::ClusterEvent::SessionDataReceived { session_id, data } => {
                    info!("Received session data for {}", session_id);
                    if let Ok(session) = serde_json::from_slice::<Session>(&data) {
                        if !cluster_state.import_session(session_id, session) {
                            warn!("Session import refused: invalid ID, duplicate or capacity");
                        }
                    }
                }
                spine_cluster::ClusterEvent::SearchRequested {
                    query,
                    request_id,
                    origin_node,
                } => {
                    info!(
                        "Distributed search {} requested from node {}",
                        request_id, origin_node
                    );

                    let mut results = Vec::new();
                    for session in cluster_state.sessions.iter() {
                        if let Some(html) = &session.current_html {
                            if html.to_lowercase().contains(&query.to_lowercase()) {
                                results.push(serde_json::json!({
                                    "session_id": session.key(),
                                    "url": session.current_url,
                                    "relevance": 0.95,
                                    "snippet": format!("Found match for '{}' in active session", query)
                                }));
                            }
                        }
                    }

                    let results_json = serde_json::Value::Array(results);
                    let cluster = cluster_state.cluster.lock().await;
                    let _ = cluster.send_search_results(origin_node, request_id, results_json);
                }
                spine_cluster::ClusterEvent::SearchResultReceived {
                    request_id,
                    results: _,
                    node_id,
                } => {
                    info!(
                        "Received search results for {} from node {}",
                        request_id, node_id
                    );
                    // In a real app, we'd push this to the client via a WebSocket or similar
                }
                spine_cluster::ClusterEvent::KnowledgeSynced {
                    key,
                    value,
                    tags,
                    origin_node,
                } => {
                    info!("Knowledge synced from node {}", origin_node);
                    // Store in a global/shared knowledge base or specific session if applicable
                    // For now, we'll store it in a "cluster_shared" session
                    let entry = KnowledgeEntry {
                        key,
                        value,
                        tags,
                        timestamp_ns: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64,
                    };
                    cluster_state
                        .knowledge_base
                        .entry("cluster_shared".to_string())
                        .or_default()
                        .push(entry);
                }
                spine_cluster::ClusterEvent::SwarmSearchRequested {
                    query,
                    depth,
                    request_id: _,
                    origin_node,
                } => {
                    info!(
                        "Legacy swarm search at depth {} requested from node {}",
                        depth, origin_node
                    );
                    let _ = query;
                    warn!("Legacy swarm search unsupported; no scout session created");
                }
                spine_cluster::ClusterEvent::TaskDelegated {
                    task: _,
                    target_agent_id,
                    origin_node,
                } => {
                    info!(
                        "Legacy task event from node {} (target: {:?}); execution unsupported",
                        origin_node, target_agent_id
                    );
                    // Handle task delegation (e.g., assign to an idle autonomous session)
                }
                spine_cluster::ClusterEvent::KnowledgeProposed {
                    proposal_id,
                    key,
                    value,
                    tags,
                    origin_node,
                } => {
                    info!(
                        "Legacy knowledge proposal {} received from node {}",
                        proposal_id, origin_node
                    );

                    // Store proposal
                    cluster_state.proposals.insert(
                        proposal_id,
                        KnowledgeProposal {
                            key: key.clone(),
                            value: value.clone(),
                            tags: tags.clone(),
                            votes: Vec::new(),
                            origin_node,
                        },
                    );

                    // No automatic confidence vote: this path has no evaluator.
                }
                spine_cluster::ClusterEvent::KnowledgeVoteReceived {
                    proposal_id,
                    voter_id,
                    approved,
                    confidence,
                } => {
                    info!(
                        "Vote received for proposal {} from node {}: approved={}, confidence={:.2}",
                        proposal_id, voter_id, approved, confidence
                    );

                    if let Some(mut proposal) = cluster_state.proposals.get_mut(&proposal_id) {
                        proposal.votes.push(KnowledgeVote {
                            voter_id,
                            approved,
                            confidence,
                        });

                        // Check if consensus reached using weighted voting
                        let cluster = cluster_state.cluster.lock().await;
                        let total_nodes = cluster.get_healthy_nodes().len() + 1; // +1 for self

                        // Compute weighted approval score using voter confidence
                        let weighted_approval: f32 = proposal
                            .votes
                            .iter()
                            .filter(|v| v.approved)
                            .map(|v| v.confidence)
                            .sum();
                        let total_confidence: f32 =
                            proposal.votes.iter().map(|v| v.confidence).sum();
                        let weighted_ratio = if total_confidence > 0.0 {
                            weighted_approval / total_confidence
                        } else {
                            0.0
                        };

                        // Log vote breakdown by voter
                        for vote in proposal.votes.iter() {
                            debug!(
                                "  Voter {}: approved={}, confidence={:.2}",
                                vote.voter_id, vote.approved, vote.confidence
                            );
                        }

                        let approved_votes = proposal.votes.iter().filter(|v| v.approved).count();
                        let threshold = cluster.get_consensus_threshold();
                        info!(
                            "Proposal {} from node {}: {}/{} votes ({:.1}% weighted approval)",
                            proposal_id,
                            proposal.origin_node,
                            approved_votes,
                            total_nodes,
                            weighted_ratio * 100.0
                        );

                        if (approved_votes as f32 / total_nodes as f32) >= threshold {
                            info!(
                                "Consensus reached for proposal {}. Committing...",
                                proposal_id
                            );
                            let _ = cluster.commit_knowledge(
                                proposal_id,
                                proposal.key.clone(),
                                proposal.value.clone(),
                                proposal.tags.clone(),
                            );
                        }
                    }
                }
                spine_cluster::ClusterEvent::KnowledgeCommitted {
                    proposal_id,
                    key,
                    value,
                    tags,
                } => {
                    info!("Knowledge proposal {} committed", proposal_id);

                    let entry = KnowledgeEntry {
                        key,
                        value,
                        tags,
                        timestamp_ns: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64,
                    };
                    cluster_state
                        .knowledge_base
                        .entry("cluster_consensus".to_string())
                        .or_default()
                        .push(entry);
                    cluster_state.proposals.remove(&proposal_id);
                }
                spine_cluster::ClusterEvent::SwarmPlanProposed { plan, origin_node } => {
                    info!(
                        "Legacy swarm plan {} proposed by node {} ({} tasks)",
                        plan.id,
                        origin_node,
                        plan.tasks.len()
                    );
                    cluster_state.plans.insert(plan.id, plan);
                }
                spine_cluster::ClusterEvent::PlanTaskUpdated {
                    plan_id,
                    task_id,
                    status,
                    result,
                    node_id,
                } => {
                    info!(
                        "Plan task {} updated by node {}: status={:?}",
                        task_id, node_id, status
                    );
                    if let Some(mut plan) = cluster_state.plans.get_mut(&plan_id) {
                        if let Some(task) = plan.tasks.iter_mut().find(|t| t.id == task_id) {
                            task.status = status;
                            task.result = result;
                        }
                    }
                }
                _ => {}
            }
        }
    });

    let addr = format!("{}:{}", config.server.host, port);
    let listener = TcpListener::bind(&addr).await?;
    info!("TCP listening on {}", addr);

    // WebSocket listener on port + offset
    let ws_port = port + config.server.ws_port_offset;
    let ws_addr = format!("{}:{}", config.server.host, ws_port);
    let ws_listener = TcpListener::bind(&ws_addr).await?;
    info!("WebSocket listening on ws://{}", ws_addr);

    #[cfg(feature = "quic")]
    anyhow::ensure!(
        !config.tls.enabled,
        "optional native QUIC cannot enforce the configured TLS policy"
    );

    // QUIC listener on port + offset
    #[cfg(feature = "quic")]
    let quic_port = port + config.server.quic_port_offset;
    #[cfg(feature = "quic")]
    {
        let quic_addr: std::net::SocketAddr = format!("{}:{}", config.server.host, quic_port)
            .parse()
            .context("invalid QUIC address")?;
        let quic_builder = spine_protocol::QuicEndpointBuilder::new();
        let quic_endpoint = quic_builder.build_server(quic_addr)?;
        info!("QUIC listening on {}", quic_addr);

        let quic_state = Arc::clone(&state);
        tokio::spawn(async move {
            while let Some(incoming) = quic_endpoint.accept().await {
                if quic_state
                    .shutting_down
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    incoming.refuse();
                    continue;
                }
                let Some(admission) = quic_state
                    .connections
                    .try_acquire(incoming.remote_address().ip())
                else {
                    incoming.refuse();
                    continue;
                };
                let quic_state = Arc::clone(&quic_state);
                tokio::spawn(async move {
                    let _admission = admission;
                    match tokio::time::timeout(std::time::Duration::from_secs(10), incoming).await {
                        Ok(Ok(connection)) => {
                            info!("New QUIC connection from {}", connection.remote_address());
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(10),
                                connection.accept_bi(),
                            )
                            .await
                            {
                                Ok(Ok((send, recv))) => {
                                    let stream = spine_transport::QuicStream::new(send, recv);
                                    serve_native(stream, quic_state).await;
                                }
                                Ok(Err(e)) => debug!("QUIC stream accept failed: {e}"),
                                Err(_) => debug!("QUIC stream accept deadline exceeded"),
                            }
                        }
                        Ok(Err(e)) => debug!("QUIC connection failed: {e}"),
                        Err(_) => debug!("QUIC connection deadline exceeded"),
                    }
                });
            }
        });
    }

    // Explicit TLS policy also applies to the WebSocket listener.
    let tls_acceptor = configured_acceptor(&config.tls)?;
    // Watchdog: periodically reap idle sessions
    let watchdog_state = Arc::clone(&state);
    let watchdog_secs = config.server.session_watchdog_secs;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(watchdog_secs));
        loop {
            interval.tick().await;
            let reaped = watchdog_state.reap_sessions();
            if !reaped.is_empty() {
                #[cfg(feature = "research")]
                {
                    let cluster = watchdog_state.cluster.lock().await;
                    for id in &reaped {
                        cluster.remove_session(id);
                    }
                }
                info!(
                    reaped = reaped.len(),
                    remaining = watchdog_state.sessions.len(),
                    "Expired detached sessions"
                );
                if let Err(error) = watchdog_state.save_sessions().await {
                    error!(%error, "Failed to persist session expiry");
                }
            }
        }
    });

    // Graceful shutdown signal
    let shutdown_state = Arc::clone(&state);
    let shutdown_timeout = std::time::Duration::from_secs(config.server.shutdown_timeout_secs);
    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    let shutdown_notify_signal = Arc::clone(&shutdown_notify);
    tokio::spawn(async move {
        let signal = async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                warn!(%error,"console shutdown signal is unavailable");
                std::future::pending::<()>().await;
            }
        };
        if std::env::var("SPINE_MANAGED_CHILD").as_deref() == Ok("1") {
            use tokio::io::AsyncReadExt;
            let mut control = tokio::io::stdin();
            // One byte or EOF from the private parent pipe requests draining.
            // Parent termination closes the pipe; no network admin endpoint is exposed.
            tokio::select! {_=signal=>{},_=control.read_u8()=>{}}
        } else {
            signal.await;
        }
        info!("Shutdown signal received — draining connections...");
        shutdown_state
            .shutting_down
            .store(true, std::sync::atomic::Ordering::SeqCst);
        // Sessions that are idle, or finish their current command, send
        // CLOSE(shutting_down). Nothing waits for a slow peer: each close is
        // bounded by the connection layer's one-second write timeout, and the
        // drain below has its own deadline.
        shutdown_state.shutdown.send_replace(true);

        // Save sessions before shutdown
        if let Err(e) = shutdown_state.save_sessions().await {
            error!("Failed to save sessions during shutdown: {}", e);
        } else {
            info!("Sessions persisted successfully");
        }

        // Wait for active connections to drain (up to timeout)
        let deadline = tokio::time::Instant::now() + shutdown_timeout;
        loop {
            let active = shutdown_state.connections.active();
            if active == 0 {
                info!("All connections drained");
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                warn!(
                    "Shutdown timeout — {} connections still active, forcing exit",
                    active
                );
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }

        info!("SPINE server shut down");
        shutdown_notify_signal.notify_one();
    });

    loop {
        tokio::select! {
            // Shutdown gate
            _ = shutdown_notify.notified() => {
                info!("Accept loop terminated by shutdown signal");
                break;
            }

            // Accept TCP connections
            result = listener.accept() => {
                let (socket, addr) = result?;

                let Some(admission) = state.connections.try_acquire(addr.ip()) else {
                    drop(socket);
                    continue;
                };

                // During shutdown the connection is still accepted: the handshake
                // runs and `serve_native` answers it with CLOSE(shutting_down).
                let span = span!(Level::INFO, "connection", remote_addr = %addr, transport = "tcp");
                let _enter = span.enter();
                info!("New TCP connection from {}", addr);

                let conn_state = Arc::clone(&state);
                let tls_acceptor = tls_acceptor.clone();

                tokio::spawn(async move {
                    let _admission = admission;
                    if let Some(acceptor) = tls_acceptor {
                        match tokio::time::timeout(std::time::Duration::from_secs(10), acceptor.accept(socket)).await {
                            Ok(Ok(tls_stream)) => {
                                serve_native(tls_stream, conn_state.clone()).await;
                            }
                            Ok(Err(e)) => debug!("TLS handshake failed: {}", e),
                            Err(_) => debug!("TLS handshake deadline exceeded"),
                        }
                    } else {
                        serve_native(socket, conn_state.clone()).await;
                    }
                });
            }

            // Accept WebSocket connections
            result = ws_listener.accept() => {
                let (socket, addr) = result?;

                let Some(admission) = state.connections.try_acquire(addr.ip()) else {
                    drop(socket);
                    continue;
                };
                info!("New WebSocket connection from {}", addr);

                let conn_state = Arc::clone(&state);
                let acceptor = tls_acceptor.clone();
                tokio::spawn(async move {
                    let _admission = admission;
                    if let Some(acceptor) = acceptor {
                        match tokio::time::timeout(std::time::Duration::from_secs(10), acceptor.accept(socket)).await {
                            Ok(Ok(stream)) => handle_websocket(stream, conn_state).await,
                            Ok(Err(e)) => debug!("WebSocket TLS handshake failed: {e}"),
                            Err(_) => debug!("WebSocket TLS handshake deadline exceeded"),
                        }
                    } else {
                        handle_websocket(socket, conn_state).await;
                    }
                });
            }
        }
    }

    // Final session save on clean shutdown
    if let Err(e) = state.save_sessions().await {
        error!("Failed to save sessions on exit: {}", e);
    }
    info!("Server exited cleanly");
    Ok(())
}

async fn handle_websocket<S>(stream: S, state: Arc<BrowserState>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        spine_transport::WebSocketServerBridge::accept(stream),
    )
    .await
    {
        Ok(Ok(bridge)) => {
            serve_native(WebSocketStream::new(bridge), state).await;
        }
        Ok(Err(e)) => debug!("WebSocket upgrade failed: {e}"),
        Err(_) => debug!("WebSocket upgrade deadline exceeded"),
    }
}

/// Runs the spine-wire v1 handshake on `stream` and, if it succeeds, the
/// session. The handshake deadline is `server.hello_timeout_secs`; a
/// client that does not complete it is sent `CLOSE(timeout)` and dropped, which
/// releases its admission when the caller's task ends.
async fn serve_native<S>(stream: S, state: Arc<BrowserState>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let deadline = std::time::Duration::from_secs(state.config.server.hello_timeout_secs);
    match Connection::server_handshake(stream, &native::local_config(), deadline).await {
        Ok(mut conn) => {
            // Admission refused after the handshake: the daemon is draining.
            if state
                .shutting_down
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                conn.close(CloseCode::ShuttingDown, "the server is shutting down")
                    .await;
                return;
            }
            handle_session(conn, state).await
        }
        Err(e) => debug!("native handshake failed: {e}"),
    }
}

/// What a connection's reader and command loop queue for its writer.
enum Outgoing {
    /// A frame to send after the state checks, for example a `RESPONSE`.
    Frame(Frame),
    /// A frame the reader produced for the peer (a `PONG`, an `overloaded`
    /// `RESPONSE`), written without consulting the in-flight table.
    Answer(Frame),
    /// A `CLOSE`; the writer sends it, shuts down, and ends the session.
    Close(CloseCode, String),
}

/// One accepted request, waiting for its turn or running.
struct Job {
    request_id: u64,
    body: Vec<u8>,
}

/// Returns when the connection has been declared over (or the signal's owner
/// is gone, which also means over).
async fn flagged(signal: &mut tokio::sync::watch::Receiver<bool>) {
    let _ = signal.wait_for(|over| *over).await.map(|_| ());
}

/// The `RESPONSE` frame for `answer`. An answer that cannot be encoded becomes
/// an `internal` error, so the request id is released and the client is not
/// left waiting.
fn response_frame(request_id: u64, answer: &Answer) -> Frame {
    let encoded = match answer {
        Ok(output) => response_ok_body(output).map_err(|e| e.to_string()),
        Err(error) => native::error_body(error).map_err(|e| e.to_string()),
    };
    let body = match encoded {
        Ok(body) => body,
        Err(message) => {
            warn!("Response could not be encoded: {message}");
            native::error_body(&fail(
                ERR_INTERNAL,
                format!("response could not be encoded: {message}"),
            ))
            .unwrap_or_else(|_| vec![0xa0])
        }
    };
    Frame {
        frame_type: FrameType::Response,
        request_id,
        body,
    }
}

/// Why a queued frame did not get written.
enum WriteFailure {
    /// The write deadline passed with the frame partly written.
    Deadline,
    /// The connection layer refused or failed the write.
    Failed(ConnectionError),
}

/// Writes one queued frame under [`native::WRITE_DEADLINE`].
async fn write_once<S>(
    writer: &mut ConnectionWriter<S>,
    frame: Frame,
    checked: bool,
) -> Result<(), WriteFailure>
where
    S: AsyncWrite + Unpin + Send,
{
    let written = tokio::time::timeout(native::WRITE_DEADLINE, async {
        if checked {
            writer.send(frame).await
        } else {
            writer.answer(frame).await
        }
    })
    .await;
    match written {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(WriteFailure::Failed(e)),
        Err(_) => Err(WriteFailure::Deadline),
    }
}

/// Writes a queued frame. Returns false when the connection is over and
/// nothing more should be written. A response that cannot be sent but leaves
/// the connection usable (for example one over the frame limit) is replaced by
/// an `internal` error, so the request id is released.
async fn write_queued<S>(writer: &mut ConnectionWriter<S>, frame: Frame, checked: bool) -> bool
where
    S: AsyncWrite + Unpin + Send,
{
    let request_id = frame.request_id;
    let is_response = frame.frame_type == FrameType::Response;
    match write_once(writer, frame, checked).await {
        Ok(()) => true,
        Err(WriteFailure::Deadline) => {
            // The stream may hold a partial frame: write nothing more.
            warn!("Write deadline exceeded; abandoning the connection");
            writer.abandon();
            false
        }
        Err(WriteFailure::Failed(e)) => {
            if writer.is_dead() {
                debug!("Failed to send frame: {e}");
                return false;
            }
            if matches!(e, ConnectionError::Unusable) {
                // The reader finished the connection (a violation, the peer
                // went away). A `CLOSE` it asked for may still be queued
                // behind this frame, so keep draining.
                debug!("Dropping a frame for a finished connection");
                return true;
            }
            if !is_response || !checked {
                debug!("Failed to send frame: {e}");
                return false;
            }
            warn!("Response could not be sent: {e}");
            let body = match native::error_body(&fail(
                ERR_INTERNAL,
                format!("response could not be sent: {e}"),
            )) {
                Ok(body) => body,
                Err(_) => return false,
            };
            let replacement = Frame {
                frame_type: FrameType::Response,
                request_id,
                body,
            };
            write_once(writer, replacement, true).await.is_ok()
        }
    }
}

/// The single owner of a connection's write half. Writes what the other two
/// loops queue, in order. It ends after a `CLOSE`, a failed or timed-out
/// write, or when every sender is gone and the queue is empty; whichever it
/// is, the connection is then over and the other loops are told so.
async fn write_responses<S>(
    mut writer: ConnectionWriter<S>,
    mut queue: tokio::sync::mpsc::Receiver<Outgoing>,
    over: Arc<tokio::sync::watch::Sender<bool>>,
) where
    S: AsyncWrite + Unpin + Send,
{
    while let Some(item) = queue.recv().await {
        let go_on = match item {
            Outgoing::Frame(frame) => write_queued(&mut writer, frame, true).await,
            Outgoing::Answer(frame) => write_queued(&mut writer, frame, false).await,
            Outgoing::Close(code, message) => {
                writer.close(code, &message).await;
                false
            }
        };
        if !go_on {
            break;
        }
    }
    over.send_replace(true);
}

/// The single owner of a connection's read half: reads frames for as long as
/// the connection lives, whatever the command loop is doing.
///
/// - `PING` and the `overloaded` refusal come back from the reader as
///   [`Incoming::Reply`] and are queued for the writer at once.
/// - A `REQUEST` the reader accepted (the table has room) goes to the command
///   loop. The queue has room for `max_inflight` jobs, and an id stays in the
///   table until its `RESPONSE` is written, so the queue cannot be full here.
/// - `STREAM_CANCEL` is delivered by the reader for an id in flight and
///   ignored: v1 defines it only for a stream, and no command streams.
/// - `PONG` answers a `PING` this daemon never sends and is ignored.
/// - A peer `CLOSE`, end of stream, or an i/o error ends the loop; a
///   violation or the idle deadline queues the `CLOSE` the reader wants first.
///
/// When the loop ends it declares the connection over, which stops the command
/// loop (a running command is dropped) and, through the closed queues, the
/// writer.
async fn read_requests<S>(
    mut reader: ConnectionReader<S>,
    queue: tokio::sync::mpsc::Sender<Outgoing>,
    jobs: tokio::sync::mpsc::Sender<Job>,
    over: Arc<tokio::sync::watch::Sender<bool>>,
    mut over_seen: tokio::sync::watch::Receiver<bool>,
    idle: std::time::Duration,
) where
    S: AsyncRead + Unpin + Send,
{
    loop {
        // `recv_timeout` suspends its deadline while a request is in flight.
        // A read dropped part-way through a frame is fine: the connection is
        // over when `over_seen` fires and is not read again.
        let received = tokio::select! {
            _ = flagged(&mut over_seen) => break,
            received = reader.recv_timeout(idle) => received,
        };
        match received {
            Ok(Some(Incoming::Frame(frame))) => match frame.frame_type {
                FrameType::Request => {
                    let job = Job {
                        request_id: frame.request_id,
                        body: frame.body,
                    };
                    if jobs.send(job).await.is_err() {
                        break;
                    }
                }
                FrameType::StreamCancel => {
                    debug!(
                        "Ignoring STREAM_CANCEL for request {}: nothing streams",
                        frame.request_id
                    );
                }
                _ => {}
            },
            Ok(Some(Incoming::Reply(frame))) => {
                if queue.send(Outgoing::Answer(frame)).await.is_err() {
                    break;
                }
            }
            Ok(None) => break,
            Err(ConnectionError::Closed(close)) => {
                debug!("Client closed the connection: {}", close.code);
                break;
            }
            Err(e) => {
                if matches!(e, ConnectionError::Timeout) {
                    debug!("Connection idle too long");
                } else {
                    error!("Connection error: {}", e);
                }
                if let Some(close) = e.close_to_send() {
                    let code = close.close_code().unwrap_or(CloseCode::ProtocolViolation);
                    let _ = queue.send(Outgoing::Close(code, close.message)).await;
                }
                break;
            }
        }
    }
    over.send_replace(true);
}

/// Runs the commands of one connection, one at a time in arrival order, and
/// queues each answer for the writer. Per-connection state (the session, the
/// rate buckets) lives here, so commands of one connection are never
/// concurrent.
///
/// Ends when the connection is over (a command that is running is dropped at
/// its next await point, and the session is detached as it is when the
/// connection ends normally), when the reader has gone and the queue is empty,
/// or on shutdown. On shutdown a command that is running finishes and is
/// answered, requests still waiting get no answer, and `CLOSE(shutting_down)`
/// goes out after the answers already queued.
async fn run_commands(
    state: Arc<BrowserState>,
    mut jobs: tokio::sync::mpsc::Receiver<Job>,
    queue: tokio::sync::mpsc::Sender<Outgoing>,
    mut over_seen: tokio::sync::watch::Receiver<bool>,
) {
    let mut session_lease: Option<SessionLease> = None;
    let tool_rate = RateLease {
        state: state.clone(),
        id: Uuid::new_v4().to_string(),
    };
    // The bucket for session commands is the connection's, so it applies
    // before a session exists and a refused command creates none.
    let command_rate = RateLease {
        state: state.clone(),
        id: Uuid::new_v4().to_string(),
    };
    let mut shutdown = state.shutdown.subscribe();

    loop {
        // Between commands: on shutdown say so and end.
        let draining = *shutdown.borrow();
        if draining {
            let _ = queue
                .send(Outgoing::Close(
                    CloseCode::ShuttingDown,
                    "the server is shutting down".to_string(),
                ))
                .await;
            break;
        }
        let job = tokio::select! {
            _ = flagged(&mut over_seen) => break,
            _ = shutdown.changed() => continue,
            job = jobs.recv() => match job {
                Some(job) => job,
                None => break,
            },
        };
        let request_id = job.request_id;
        let answer = tokio::select! {
            _ = flagged(&mut over_seen) => break,
            answer = answer_job(&state, &mut session_lease, &tool_rate, &command_rate, job) => answer,
        };
        if queue
            .send(Outgoing::Frame(response_frame(request_id, &answer)))
            .await
            .is_err()
        {
            break;
        }
    }
}

/// The answer to one request: decodes it, applies the connection's rate
/// limits, creates the session on first use and runs the command.
async fn answer_job(
    state: &Arc<BrowserState>,
    session_lease: &mut Option<SessionLease>,
    tool_rate: &RateLease,
    command_rate: &RateLease,
    job: Job,
) -> Answer {
    let request_id = job.request_id;
    let command = match native::decode_command(&job.body) {
        Ok(command) => command,
        // C.7: a well-formed frame whose request cannot be served is an
        // error response, not the end of the connection. The code says
        // whether the command is unknown or its fields are wrong.
        Err(e) => return Err(e.to_wire()),
    };
    match command {
        BrowserCommand::CapabilityQuery(query) => {
            let capabilities = spine_core::tools::capabilities()
                .into_iter()
                .filter(|capability| match &query.selector {
                    spine_protocol::CapabilitySelector::All => true,
                    spine_protocol::CapabilitySelector::Exact(uri) => &capability.uri == uri,
                    spine_protocol::CapabilitySelector::Prefix(prefix) => {
                        capability.uri.starts_with(prefix)
                    }
                    spine_protocol::CapabilitySelector::Semantic { .. } => false,
                })
                .collect();
            let agent_id = match &state.mesh_names {
                Some(mesh) => spine_name::SpineUri::did(*mesh.local_key().as_bytes()).to_string(),
                None => "spine-core:local-unattested".into(),
            };
            let advertisement = spine_protocol::CapabilityAdvertisement {
                agent_id,
                capabilities,
            };
            Ok(Output::Capabilities(advertisement))
        }
        BrowserCommand::ToolCall(call) => {
            let outcome = if state.check_rate_limit(&tool_rate.id) {
                spine_core::tools::invoke(call)
            } else {
                ToolOutcome::Err {
                    code: ERR_RATE_LIMIT.into(),
                    message: "tool request rate exceeded".into(),
                    details: None,
                }
            };
            outcome.into_reply().into_result().map(Output::Json)
        }
        command => {
            let req_span = span!(Level::INFO, "request", request_id = request_id);
            debug!("Received request {request_id}");

            // Rate limit first: a refused command creates no session.
            if !state.check_rate_limit(&command_rate.id) {
                return Err(fail(ERR_RATE_LIMIT, "Rate limit exceeded".to_string()));
            }

            if session_lease.is_none() {
                *session_lease = state.create_session();
                if let Some(lease) = session_lease.as_ref() {
                    #[cfg(feature = "research")]
                    state
                        .cluster
                        .lock()
                        .await
                        .register_local_session(lease.id.clone());
                    #[cfg(not(feature = "research"))]
                    let _ = lease;
                } else {
                    // No close code means "busy", so the connection stays
                    // open: the client may retry, or hang up.
                    return Err(fail(
                        ERR_SESSION_CAPACITY,
                        "Server busy: maximum sessions reached".into(),
                    ));
                }
            }
            let id = match session_lease.as_ref() {
                Some(lease) => lease.id.clone(),
                None => {
                    return Err(fail(
                        ERR_SESSION_CAPACITY,
                        "Server busy: maximum sessions reached".into(),
                    ))
                }
            };

            handle_command(state, &id, command, request_id)
                .instrument(req_span)
                .await
        }
    }
}

/// Serves one established native connection, with reading, executing and
/// writing as three independent loops:
///
/// - a reader task (`read_requests`) that never waits for a command, so a
///   `PING` is answered, a request over `max_inflight` is refused with
///   `overloaded`, and a closed connection is noticed while a command runs;
/// - the command loop (`run_commands`), run inline, which executes the
///   connection's commands one at a time in arrival order;
/// - a writer task (`write_responses`) that owns the write half.
///
/// The channels between them are bounded: at most `max_inflight` requests are
/// accepted at a time, and the writer's queue holds their answers plus a few
/// control frames, with back-pressure on the reader beyond that. Both spawned
/// tasks live in a `JoinSet` that is drained before this function returns, so
/// no task outlives the connection, and dropping this future aborts them.
async fn handle_session<S>(conn: Connection<S>, state: Arc<BrowserState>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let idle = std::time::Duration::from_secs(state.config.server.idle_timeout_secs);
    let (reader, writer) = conn.split();
    let capacity = reader.max_inflight().max(1) as usize;
    let (queue_tx, queue_rx) = tokio::sync::mpsc::channel::<Outgoing>(capacity + 16);
    let (jobs_tx, jobs_rx) = tokio::sync::mpsc::channel::<Job>(capacity);
    let (over_tx, over_rx) = tokio::sync::watch::channel(false);
    let over: Arc<tokio::sync::watch::Sender<bool>> = Arc::new(over_tx);

    let mut tasks: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();
    tasks.spawn(write_responses(writer, queue_rx, Arc::clone(&over)));
    tasks.spawn(read_requests(
        reader,
        queue_tx.clone(),
        jobs_tx,
        Arc::clone(&over),
        over.subscribe(),
        idle,
    ));

    run_commands(state, jobs_rx, queue_tx, over_rx).await;
    // The command loop ended (shutdown, or the connection is over): the reader
    // stops, which closes the writer's queue after what is queued is written.
    over.send_replace(true);
    while tasks.join_next().await.is_some() {}
}
