//! Driving name resolution over a live mesh.
//!
//! [`crate::naming`] supplies the protocol logic — which peers to ask, what to
//! answer, when a lookup has converged — as pure state machines. This module
//! supplies the part that makes them *run*: envelope dispatch, request/response
//! correlation, timeouts, and the bridge between the two routing spaces SPINE
//! now has.
//!
//! ## Two routing spaces, one bridge
//!
//! The mesh routes by [`AgentId`] (a UUID). The namespace routes by
//! [`NameKey`] (a 256-bit keyspace point). A DHT lookup yields keyspace
//! neighbours, but an envelope has to be addressed to an agent — so something
//! has to relate them.
//!
//! That bridge is nearly free, because a SPINE agent's keyspace position *is*
//! its Ed25519 public key, and [`PublicIdentity`] already carries the key
//! alongside the agent id. Registering a peer therefore populates both
//! directions at once, and a node that cannot be mapped is simply one the mesh
//! has not authenticated yet — it is skipped rather than guessed at.
//!
//! ## Sending
//!
//! `MeshNode` builds and signs envelopes but never touches a socket; transmission
//! belongs to whatever transport the deployment chose. [`NameTransport`] keeps
//! that seam: implement it over TCP, WebSocket, QUIC, or an in-process channel,
//! and the resolution machinery is unchanged. The tests use an in-process
//! implementation to exercise a real multi-node resolution end to end.

use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};

use spine_name::{NameKey, NameRecord, NodeInfo, SpineUri};

use crate::identity::PublicIdentity;
use crate::mesh::{MeshEnvelope, MeshNode, MeshPayload};
use crate::naming::{
    KeyspacePeer, LookupOutcome, NameService, ResolveQuery, ResolveResponse, K,
};
use crate::AgentId;

/// Default time a lookup waits before giving up.
pub const DEFAULT_LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

/// How a driver actually puts an envelope on the wire.
///
/// The mesh signs envelopes but does not own a socket, so this is the seam
/// between resolution logic and transport. Implementations must be cheap to
/// clone-share and safe to call concurrently.
#[async_trait]
pub trait NameTransport: Send + Sync {
    /// Deliver an envelope to the agent it is addressed to.
    ///
    /// Returning `Err` marks that peer unreachable for the current lookup; it
    /// does not fail the lookup, which continues with its other candidates.
    async fn send(&self, envelope: MeshEnvelope) -> Result<(), NameMeshError>;

    /// Deliver an envelope to a bare address, to a peer whose mesh identity is
    /// not yet known.
    ///
    /// This exists solely for bootstrap. Every other send is addressed to an
    /// [`AgentId`], but the first contact with a seed node is by definition made
    /// before its identity is known — that is the fact bootstrap has to
    /// establish. The connection is retained so the reply can arrive on it, in
    /// keeping with the rule that answers travel back the way the question came.
    ///
    /// Transports that cannot dial by address inherit a refusal, so an
    /// in-process or relay-only transport is not obliged to pretend.
    async fn send_to(&self, endpoint: &str, envelope: MeshEnvelope) -> Result<(), NameMeshError> {
        let _ = envelope;
        Err(NameMeshError::Transport(format!(
            "this transport cannot dial `{endpoint}` by address"
        )))
    }

    /// Learn where a peer can be reached.
    ///
    /// The resolver discovers addresses — from a hello, an ack, or a referral
    /// mid-lookup — but a transport keeps its own address book, and until it is
    /// told, a peer the resolver considers known is still one the transport
    /// cannot dial. That split is precisely what made keyspace referrals useless
    /// before: the lookup would add a peer, fail to reach it, and mark it
    /// unreachable.
    ///
    /// Implementations should take the first endpoint they can actually use and
    /// ignore the rest, since a peer may advertise addresses in forms this
    /// transport does not speak.
    async fn learn(&self, agent: AgentId, endpoints: &[String]) {
        let _ = (agent, endpoints);
    }

    /// Drop a connection opened by [`NameTransport::send_to`].
    ///
    /// Called once bootstrap has learned the peer's identity, after which the
    /// peer is reachable through ordinary pooled dialing and the provisional
    /// connection is redundant.
    async fn release(&self, endpoint: &str) {
        let _ = endpoint;
    }
}

/// Failures specific to driving resolution over a mesh.
#[derive(Debug, thiserror::Error)]
pub enum NameMeshError {
    #[error("transport failure: {0}")]
    Transport(String),

    #[error("lookup timed out after {0:?}")]
    Timeout(Duration),

    #[error("name did not resolve: {0}")]
    NotFound(String),

    #[error("naming error: {0}")]
    Name(#[from] spine_name::NameError),
}

/// Statistics worth watching: a resolver that answers mostly from cache is the
/// difference between a swarm that scales and one that floods its own mesh.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolverMetrics {
    pub lookups_started: u64,
    pub lookups_resolved: u64,
    pub lookups_timed_out: u64,
    pub requests_answered: u64,
    pub announcements_accepted: u64,
    pub announcements_rejected: u64,
    pub unroutable_peers: u64,
    /// Bootstrap hellos answered for other nodes.
    pub greetings_answered: u64,
    /// Hellos or acks whose signature did not match the key they carried.
    pub greetings_rejected: u64,
    /// Directed copies of records sent to the nodes closest to their keys.
    pub replicas_sent: u64,
    /// Replicas this node could not deliver.
    pub replicas_failed: u64,
    /// Records dropped because they expired.
    pub records_expired: u64,
    /// Announcements declined for want of room or proximity.
    ///
    /// Deliberately not counted as `announcements_rejected`: nothing was wrong
    /// with these records or with the peers that sent them, so a rise here means
    /// something quite different from a rise in forgeries.
    pub announcements_declined: u64,
}

/// A peer learned by dialing a seed address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapPeer {
    pub agent_id: AgentId,
    pub key: NameKey,
    pub endpoints: Vec<String>,
    /// How many further contacts the seed handed back.
    pub referrals: usize,
}

/// What a [`MeshNameResolver::bootstrap`] achieved.
#[derive(Debug, Clone, Default)]
pub struct BootstrapReport {
    /// Seeds that answered, with the identity each proved.
    pub reached: Vec<BootstrapPeer>,
    /// Seeds that did not answer, with why.
    pub failed: Vec<(String, String)>,
    /// Addressable peers known once the self-lookup finished.
    pub peers_after: usize,
}

impl BootstrapReport {
    /// Whether the node has an entry point into the DHT.
    ///
    /// One reachable seed is enough: Kademlia converges from any single honest
    /// contact. This is deliberately not "all seeds answered" — requiring that
    /// would make a node refuse to start because a spare seed was down.
    pub fn is_connected(&self) -> bool {
        !self.reached.is_empty()
    }
}

/// Where a record ended up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplicationReport {
    /// Peers that accepted a directed copy.
    pub sent: usize,
    /// Peers the copy could not be delivered to.
    pub failed: usize,
    /// Whether the fan-out fell back to a broadcast because no keyspace peer
    /// could be addressed.
    pub broadcast: bool,
}

impl ReplicationReport {
    /// How many copies exist beyond the publisher's own.
    ///
    /// A broadcast counts as none: it may well have reached peers, but not
    /// peers chosen for their position, so nothing about the record's
    /// durability follows from it.
    pub fn replicas(&self) -> usize {
        self.sent
    }

    /// Whether the record survives this node going away.
    pub fn is_durable(&self) -> bool {
        self.sent > 0
    }
}

/// How much work one maintenance pass may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaintenancePolicy {
    /// How close to expiry a name must be to be reported as lapsing.
    pub lapse_window_secs: u64,
    /// Most records to re-offer in a single pass.
    ///
    /// Each one costs a keyspace walk, so an unbounded pass on a node with a
    /// large store is a self-inflicted flood every tick. What does not fit
    /// carries over: passes resume where the last stopped, so a store larger
    /// than the budget is covered across several ticks rather than having its
    /// tail starved.
    pub max_records: usize,
}

impl Default for MaintenancePolicy {
    fn default() -> Self {
        Self {
            lapse_window_secs: 900,
            max_records: 64,
        }
    }
}

/// What one maintenance pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaintenanceReport {
    /// Records dropped because their TTL had run out.
    pub expired: usize,
    /// Records re-offered to the nodes currently closest to their keys.
    pub refreshed: usize,
    /// Directed copies sent across all refreshed records.
    pub replicas_sent: usize,
    /// Records held but not this node's to keep alive, because K nearer nodes
    /// exist. Reported rather than silently dropped from the count: a rising
    /// number here means this node is carrying copies lookups will never reach.
    pub not_ours: usize,
    /// Records this node is responsible for that the budget deferred to the
    /// next pass. Reported so a bounded pass never reads as an exhaustive one.
    pub deferred: usize,
    /// Names about to lapse. Only the holder of the signing key can renew one,
    /// so this is a report for the layer above, not something the pass fixes.
    pub lapsing: Vec<SpineUri>,
}

/// A [`NameService`] wired to a live mesh.
pub struct MeshNameResolver {
    service: Mutex<NameService>,
    node: Arc<MeshNode>,
    transport: Arc<dyn NameTransport>,
    /// Addresses this node advertises to peers it introduces itself to.
    endpoints: Mutex<Vec<String>>,
    /// Lookups awaiting an outcome, by request id.
    waiters: DashMap<u64, oneshot::Sender<LookupOutcome>>,
    /// Bootstrap handshakes awaiting an ack, by the endpoint dialed.
    greetings: DashMap<String, oneshot::Sender<BootstrapPeer>>,
    /// Last record key a maintenance pass re-offered, so the next one resumes
    /// after it rather than restarting at the same prefix.
    maintenance_cursor: Mutex<Option<NameKey>>,
    timeout: Duration,
    metrics: Mutex<ResolverMetrics>,
    /// Injected clock (Unix seconds), so record freshness is testable without
    /// waiting out real TTLs.
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
}

impl std::fmt::Debug for MeshNameResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeshNameResolver")
            .field("agent_id", self.node.agent_id())
            .field("pending_lookups", &self.waiters.len())
            .field("pending_greetings", &self.greetings.len())
            .finish_non_exhaustive()
    }
}

impl MeshNameResolver {
    /// Build a resolver for `node`, sending through `transport`.
    pub fn new(node: Arc<MeshNode>, transport: Arc<dyn NameTransport>) -> Self {
        let service = NameService::new(node.name_key());
        Self {
            service: Mutex::new(service),
            node,
            transport,
            endpoints: Mutex::new(Vec::new()),
            waiters: DashMap::new(),
            greetings: DashMap::new(),
            maintenance_cursor: Mutex::new(None),
            timeout: DEFAULT_LOOKUP_TIMEOUT,
            metrics: Mutex::new(ResolverMetrics::default()),
            clock: Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            }),
        }
    }

    /// Override the lookup timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Override the clock — for tests and deterministic replay.
    pub fn with_clock(mut self, clock: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    /// Current time according to the injected clock.
    pub fn now(&self) -> u64 {
        (self.clock)()
    }

    /// This node's keyspace position.
    pub fn local_key(&self) -> NameKey {
        self.node.name_key()
    }

    /// Register a peer in both routing spaces at once.
    ///
    /// Takes a [`PublicIdentity`] rather than a bare key so the two ids can
    /// never drift apart: they come from the same authenticated object.
    pub async fn register_peer(&self, identity: &PublicIdentity, endpoints: Vec<String>, now: u64) {
        let Ok(key_bytes) = <[u8; 32]>::try_from(identity.public_key.as_slice()) else {
            // Not an Ed25519 key: it has no place in the keyspace, and inventing
            // one would put the peer somewhere nothing could find it.
            return;
        };
        let key = NameKey::from_bytes(key_bytes);
        self.transport.learn(identity.agent_id, &endpoints).await;
        self.service
            .lock()
            .await
            .add_peer(NodeInfo::new(key, endpoints, now), identity.agent_id);
    }

    /// Record a peer in both routing spaces *and* in the transport's address
    /// book, so that knowing about it and being able to reach it stay the same
    /// thing.
    async fn adopt_peer(&self, key: NameKey, agent_id: AgentId, endpoints: Vec<String>, now: u64) {
        self.transport.learn(agent_id, &endpoints).await;
        self.service
            .lock()
            .await
            .add_peer(NodeInfo::new(key, endpoints, now), agent_id);
    }

    /// Set the addresses this node advertises when introducing itself.
    ///
    /// A node with none can still bootstrap *outward* — it will learn peers and
    /// resolve names — but no peer it meets will be able to dial it back, so it
    /// participates in the DHT as a client rather than as a contact. That is the
    /// right default for an agent behind NAT and the wrong one for a seed.
    pub async fn set_endpoints(&self, endpoints: Vec<String>) {
        *self.endpoints.lock().await = endpoints;
    }

    /// The addresses this node advertises.
    pub async fn endpoints(&self) -> Vec<String> {
        self.endpoints.lock().await.clone()
    }

    /// Enter the DHT knowing nothing but a list of addresses.
    ///
    /// A Kademlia node converges on the right neighbours from any single honest
    /// contact, but it cannot acquire the first one by routing — routing is what
    /// having a contact enables. Every peer registration before this took a
    /// [`PublicIdentity`], which a node reading a config file does not have and
    /// cannot derive from an address. So bootstrap is the one exchange that runs
    /// the other way round: dial the address, and let the peer prove which
    /// identity is listening there.
    ///
    /// Three steps per seed, all of them necessary:
    ///
    /// 1. Dial the address and send a [`NameHello`] carrying our own key.
    /// 2. Take the ack, whose signature verifies against the key *it* carries —
    ///    which is also the seed's keyspace position, so a seed cannot claim a
    ///    position it cannot sign for.
    /// 3. Absorb the contacts the ack referred us to.
    ///
    /// Then one self-lookup: asking the keyspace for our own position is the
    /// standard way to fill the buckets nearest us, and it costs one walk.
    ///
    /// Seeds are dialed concurrently and failures are collected rather than
    /// raised, because a node that refuses to start over one dead seed is worse
    /// than one that starts with the others.
    pub async fn bootstrap(&self, seeds: &[String]) -> BootstrapReport {
        let mut report = BootstrapReport::default();
        if seeds.is_empty() {
            report.peers_after = self.peer_count().await;
            return report;
        }

        let greetings = seeds.iter().map(|endpoint| async move {
            (endpoint.clone(), self.greet(endpoint).await)
        });

        for (endpoint, result) in futures::future::join_all(greetings).await {
            match result {
                Ok(peer) => report.reached.push(peer),
                Err(e) => report.failed.push((endpoint, e.to_string())),
            }
        }

        // A walk toward our own key with no contact to start from is a no-op, so
        // skip it rather than spend the lookup timeout proving that.
        if report.is_connected() {
            self.refresh_self().await;
        }

        report.peers_after = self.peer_count().await;
        report
    }

    /// Introduce this node to one seed address and wait for it to answer.
    async fn greet(&self, endpoint: &str) -> Result<BootstrapPeer, NameMeshError> {
        let (tx, rx) = oneshot::channel();
        self.greetings.insert(endpoint.to_string(), tx);

        let hello = self.node.name_hello(self.endpoints().await);
        if let Err(e) = self.transport.send_to(endpoint, hello).await {
            self.greetings.remove(endpoint);
            self.transport.release(endpoint).await;
            return Err(e);
        }

        let outcome = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(peer)) => Ok(peer),
            // The sender was dropped without a value: the resolver went away.
            Ok(Err(_)) => Err(NameMeshError::Transport(format!(
                "greeting to {endpoint} was abandoned"
            ))),
            Err(_) => {
                self.greetings.remove(endpoint);
                Err(NameMeshError::Timeout(self.timeout))
            }
        };

        // Either way the provisional connection has done its job: the peer is
        // now reachable by identity through ordinary pooled dialing.
        self.transport.release(endpoint).await;
        outcome
    }

    /// Walk the keyspace toward our own position, to fill the nearest buckets.
    ///
    /// The lookup is expected to find no record — nobody has published one under
    /// this node's key — and that is fine: the point is the referrals collected
    /// on the way, which [`NameService::on_response`] absorbs regardless of
    /// whether the walk answers.
    async fn refresh_self(&self) {
        let own = SpineUri::did(*self.node.name_key().as_bytes());
        let _ = self.resolve(&own).await;
    }

    /// Publish a signed record: store it locally, then place copies at the nodes
    /// closest to its key.
    ///
    /// The report says how many copies landed. A publisher that cares whether
    /// its name outlives its own process should check it — `sent == 0` means the
    /// record exists in exactly one place.
    pub async fn publish(&self, record: NameRecord) -> Result<ReplicationReport, NameMeshError> {
        self.service.lock().await.publish(record.clone())?;
        Ok(self.replicate(&record).await)
    }

    /// Publish locally without announcing — for names that should be resolvable
    /// but not advertised.
    pub async fn publish_local(&self, record: NameRecord) -> Result<(), NameMeshError> {
        self.service.lock().await.publish(record)?;
        Ok(())
    }

    /// Place copies of a record at the nodes closest to its key.
    ///
    /// Walks the keyspace first rather than trusting the local routing table:
    /// the nodes that joined nearest this key since the last walk are exactly
    /// the ones a stale table omits, and they are the ones that will be asked
    /// for the record.
    pub async fn replicate(&self, record: &NameRecord) -> ReplicationReport {
        let key = record.name.key();
        let targets = self.find_node(key).await;

        let mut report = ReplicationReport::default();

        if targets.is_empty() {
            // No addressable keyspace peer. Broadcasting is not replication, but
            // it is what a node with a mesh connection and no routing table can
            // still do, and it warms whatever caches are listening. The report
            // says plainly that nothing durable happened.
            if let Ok(envelope) = self.node.announce_name(record.clone()) {
                report.broadcast = self.transport.send(envelope).await.is_ok();
            }
            return report;
        }

        let mut sends = Vec::new();
        for peer in &targets {
            // announce_name_to re-verifies, so an invalid record never reaches
            // the wire even if it somehow entered the store.
            let Ok(envelope) = self.node.announce_name_to(peer.agent_id, record.clone()) else {
                continue;
            };
            let transport = self.transport.clone();
            sends.push(async move { transport.send(envelope).await });
        }

        for result in futures::future::join_all(sends).await {
            match result {
                Ok(()) => report.sent += 1,
                Err(_) => report.failed += 1,
            }
        }

        let mut m = self.metrics.lock().await;
        m.replicas_sent += report.sent as u64;
        m.replicas_failed += report.failed as u64;
        report
    }

    /// Find the addressable peers closest to a keyspace point.
    ///
    /// Kademlia's FIND_NODE. The walk's job is to converge on the neighbourhood;
    /// the peers are then read back out of the routing table, which absorbed
    /// every referral the walk collected on the way.
    pub async fn find_node(&self, key: NameKey) -> Vec<KeyspacePeer> {
        let _ = self.run_lookup(ResolveQuery::Node(key)).await;
        self.service.lock().await.closest_peers(&key, K)
    }

    /// Re-offer held records to the nodes now closest to them, and drop the dead.
    ///
    /// This is what keeps a record alive under churn. A record is stored at the
    /// K closest nodes *at the time it was published*; nodes then join, leave,
    /// and fail, and without a periodic re-offer the copies drift away from the
    /// keyspace position that lookups actually converge on.
    ///
    /// Deliberately not the same thing as renewal. A record's expiry is signed
    /// into it, so re-announcing one cannot extend its life — only its holder's
    /// key can do that. Names close to lapsing come back in the report instead.
    pub async fn maintain(&self, now: u64, policy: MaintenancePolicy) -> MaintenanceReport {
        let mut report = MaintenanceReport::default();

        let (expired, held, mine, lapsing) = {
            let mut service = self.service.lock().await;
            let expired = service.sweep(now);
            (
                expired,
                service.record_count(),
                service.records_to_maintain(now),
                service.lapsing(now, policy.lapse_window_secs),
            )
        };
        report.expired = expired;
        report.lapsing = lapsing;
        report.not_ours = held.saturating_sub(mine.len());

        // Resume after the last key the previous pass handled, so a store larger
        // than the budget is covered over several passes instead of the same
        // prefix being re-offered forever.
        let start = {
            let cursor = self.maintenance_cursor.lock().await;
            match &*cursor {
                Some(last) => mine
                    .iter()
                    .position(|r| r.name.key().as_bytes() > last.as_bytes())
                    .unwrap_or(0),
                None => 0,
            }
        };

        let budget = policy.max_records.min(mine.len());
        report.deferred = mine.len().saturating_sub(budget);

        let mut last_key = None;
        // One record at a time: each re-offer runs its own keyspace walk, and
        // firing them concurrently would have a node with a full store flood the
        // mesh with lookups every maintenance tick.
        for offset in 0..budget {
            let record = &mine[(start + offset) % mine.len()];
            last_key = Some(record.name.key());
            let result = self.replicate(record).await;
            if result.sent > 0 {
                report.refreshed += 1;
                report.replicas_sent += result.sent;
            }
        }

        if last_key.is_some() {
            *self.maintenance_cursor.lock().await = last_key;
        }

        self.metrics.lock().await.records_expired += expired as u64;
        report
    }

    /// Resolve a name, walking the mesh if it is not held locally.
    pub async fn resolve(&self, uri: &SpineUri) -> Result<NameRecord, NameMeshError> {
        match self.run_lookup(ResolveQuery::Name(uri.clone())).await? {
            LookupOutcome::Found(record) => Ok(*record),
            _ => Err(NameMeshError::NotFound(uri.to_string())),
        }
    }

    /// Find providers of a capability across the mesh.
    pub async fn find_providers(&self, term: &str) -> Result<Vec<NameRecord>, NameMeshError> {
        match self
            .run_lookup(ResolveQuery::Capability(term.to_string()))
            .await?
        {
            LookupOutcome::Providers(providers) => Ok(providers),
            _ => Ok(Vec::new()),
        }
    }

    /// Start a lookup, dispatch its first wave, and await the outcome.
    async fn run_lookup(&self, query: ResolveQuery) -> Result<LookupOutcome, NameMeshError> {
        let now = self.now();

        let (request_id, wave) = {
            let mut service = self.service.lock().await;
            // A node that already holds the answer has no business walking the
            // keyspace for it. This is the hot path once a cache is warm.
            if let Some(outcome) = service.resolve_locally(&query, now) {
                return Ok(outcome);
            }
            service.start_lookup(query)
        };
        self.metrics.lock().await.lookups_started += 1;

        // A lookup that converged from local knowledge alone never goes to the
        // wire — the common case once a cache is warm.
        if wave.is_empty() {
            let outcome = {
                let mut service = self.service.lock().await;
                service.finish_lookup(request_id)
            };
            return Ok(outcome.unwrap_or(LookupOutcome::NotFound));
        }

        let (tx, rx) = oneshot::channel();
        self.waiters.insert(request_id, tx);
        self.dispatch(request_id, wave).await;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(outcome)) => {
                self.metrics.lock().await.lookups_resolved += 1;
                Ok(outcome)
            }
            // Sender dropped: the lookup ended without an outcome being posted.
            Ok(Err(_)) => {
                self.cleanup(request_id).await;
                Ok(LookupOutcome::NotFound)
            }
            Err(_) => {
                self.waiters.remove(&request_id);
                self.cleanup(request_id).await;
                self.metrics.lock().await.lookups_timed_out += 1;
                Err(NameMeshError::Timeout(self.timeout))
            }
        }
    }

    /// Send a resolution request to each node in a wave.
    async fn dispatch(&self, request_id: u64, wave: Vec<NodeInfo>) {
        // Take the query and every node's mesh identity under one lock, rather
        // than re-locking per node in the loop below.
        let (query, addressed) = {
            let service = self.service.lock().await;
            let query = match service.lookup_query(request_id) {
                Some(q) => q,
                None => return,
            };
            let addressed: Vec<(NodeInfo, Option<AgentId>)> = wave
                .into_iter()
                .map(|n| {
                    let agent = service.agent_for(&n.id);
                    (n, agent)
                })
                .collect();
            (query, addressed)
        };

        let mut unreachable = Vec::new();
        let mut sends = Vec::new();

        for (node, agent) in addressed {
            // A keyspace neighbour we cannot map to a mesh identity is not
            // addressable. Treat it as a timeout so the lookup stops waiting on
            // it rather than stalling.
            let Some(agent_id) = agent else {
                unreachable.push(node.id);
                continue;
            };
            let envelope = self
                .node
                .name_resolve_request(agent_id, request_id, query.clone());
            let transport = self.transport.clone();
            let key = node.id;
            sends.push(async move { (key, transport.send(envelope).await) });
        }

        // Issue the whole wave concurrently. Kademlia's α exists to overlap
        // slow peers; awaiting each send in turn would serialize exactly the
        // parallelism the parameter is for, and let one unreachable peer's
        // connect timeout consume the entire lookup budget.
        let mut delivered = 0;
        for (key, result) in futures::future::join_all(sends).await {
            match result {
                Ok(()) => delivered += 1,
                Err(_) => unreachable.push(key),
            }
        }

        if !unreachable.is_empty() {
            self.metrics.lock().await.unroutable_peers += unreachable.len() as u64;
            let follow_up = {
                let mut service = self.service.lock().await;
                for id in &unreachable {
                    service.mark_unreachable(request_id, id);
                }
                service.next_wave(request_id)
            };
            if !follow_up.is_empty() {
                Box::pin(self.dispatch(request_id, follow_up)).await;
                return;
            }
        }

        // Nothing was delivered and nothing else to try: settle now rather than
        // leaving the caller to wait out the full timeout.
        if delivered == 0 {
            self.settle(request_id).await;
        }
    }

    /// Process one inbound envelope, sending whatever it calls for.
    ///
    /// Returns `true` if the envelope was a naming message. Anything else is
    /// left untouched for the caller's own mesh handling.
    pub async fn handle_envelope(&self, envelope: &MeshEnvelope, now: u64) -> bool {
        self.handle_envelope_with_reply(envelope, now, None).await
    }

    /// Process an inbound envelope, answering over `reply` when one is given.
    ///
    /// A connection-oriented transport must answer a request on the socket it
    /// arrived on. Routing the reply through the general address book instead
    /// would require every responder to have been separately introduced to
    /// every asker — which is both a bootstrapping problem and wrong through
    /// NAT, where the asker has no dialable address at all.
    pub async fn handle_envelope_with_reply(
        &self,
        envelope: &MeshEnvelope,
        now: u64,
        reply: Option<&dyn NameTransport>,
    ) -> bool {
        match &envelope.payload {
            MeshPayload::NameAnnounce(announced) => {
                // accept_announcement verifies and applies admission control, so
                // a forgery is counted and dropped rather than becoming servable
                // state, and a record for a key this node has no business
                // holding is declined rather than stored.
                let outcome = self
                    .service
                    .lock()
                    .await
                    .accept_announcement(announced.record.clone());
                let mut m = self.metrics.lock().await;
                match outcome {
                    Ok(spine_name::PutOutcome::Rejected) => m.announcements_declined += 1,
                    Ok(_) => m.announcements_accepted += 1,
                    Err(_) => m.announcements_rejected += 1,
                }
                true
            }

            MeshPayload::NameResolveRequest(request) => {
                let response = {
                    let service = self.service.lock().await;
                    service.handle_request(request, now)
                };
                self.metrics.lock().await.requests_answered += 1;
                let answer = self.node.name_resolve_response(envelope.from, response);
                match reply {
                    Some(back) => {
                        let _ = back.send(answer).await;
                    }
                    None => {
                        let _ = self.transport.send(answer).await;
                    }
                }
                true
            }

            MeshPayload::NameResolveResponse(response) => {
                self.absorb_response(response).await;
                true
            }

            MeshPayload::NameHello(hello) => {
                // The sender is unknown by construction, so the only thing that
                // makes this more than an assertion is the signature over the
                // key the hello carries.
                if !MeshNode::verify_carried_key(envelope, &hello.public_key) {
                    self.metrics.lock().await.greetings_rejected += 1;
                    return true;
                }

                let key = NameKey::from_bytes(hello.public_key);
                self.adopt_peer(key, envelope.from, hello.endpoints.clone(), now)
                    .await;
                // Answer with *the newcomer's* neighbours, not ours: those are
                // the contacts that belong in its own buckets.
                let closer = self.service.lock().await.peers_for_newcomer(&key);
                self.metrics.lock().await.greetings_answered += 1;

                let ack = self
                    .node
                    .name_hello_ack(envelope.from, self.endpoints().await, closer);
                match reply {
                    Some(back) => {
                        let _ = back.send(ack).await;
                    }
                    None => {
                        let _ = self.transport.send(ack).await;
                    }
                }
                true
            }

            MeshPayload::NameHelloAck(ack) => {
                if !MeshNode::verify_carried_key(envelope, &ack.public_key) {
                    self.metrics.lock().await.greetings_rejected += 1;
                    return true;
                }

                let key = NameKey::from_bytes(ack.public_key);
                self.adopt_peer(key, envelope.from, ack.endpoints.clone(), now)
                    .await;
                // The referrals are as useful as the seed itself: they are what
                // turns one reachable address into a routing table.
                for peer in &ack.closer {
                    self.adopt_peer(
                        peer.info.id,
                        peer.agent_id,
                        peer.info.endpoints.clone(),
                        peer.info.last_seen,
                    )
                    .await;
                }

                // Wake whichever bootstrap dial was waiting on this address. The
                // ack does not echo the endpoint we dialed, so match on the
                // endpoints the peer claims, falling back to the sole greeting
                // in flight when a seed advertises an address that differs from
                // the one we reached it on (NAT, or a seed listing its public
                // name while we dialed it on a LAN address).
                let waiter = ack
                    .endpoints
                    .iter()
                    .find_map(|e| self.greetings.remove(e))
                    .or_else(|| {
                        if self.greetings.len() != 1 {
                            return None;
                        }
                        // Clone the key out before removing: holding the
                        // iterator's guard across the remove would deadlock.
                        let only = self.greetings.iter().next().map(|e| e.key().clone())?;
                        self.greetings.remove(&only)
                    });
                if let Some((_, tx)) = waiter {
                    let _ = tx.send(BootstrapPeer {
                        agent_id: envelope.from,
                        key,
                        endpoints: ack.endpoints.clone(),
                        referrals: ack.closer.len(),
                    });
                }
                true
            }

            _ => false,
        }
    }

    /// Feed a response into its lookup and continue or finish it.
    async fn absorb_response(&self, response: &ResolveResponse) {
        let request_id = response.request_id;

        // Teach the transport how to reach everyone this response referred us
        // to, before the next wave tries to dial them. Without this the walk
        // would stop at the peers we were manually introduced to, which is no
        // walk at all.
        for peer in &response.closer {
            self.transport
                .learn(peer.agent_id, &peer.info.endpoints)
                .await;
        }

        let (next_wave, done) = {
            let mut service = self.service.lock().await;
            let wave = service.on_response(response);
            let done = service.lookup_outcome(request_id).is_some();
            (wave, done)
        };

        if done {
            self.settle(request_id).await;
            return;
        }
        if !next_wave.is_empty() {
            self.dispatch(request_id, next_wave).await;
        }
    }

    /// Complete a lookup and hand its outcome to whoever is waiting.
    async fn settle(&self, request_id: u64) {
        let outcome = {
            let mut service = self.service.lock().await;
            service.finish_lookup(request_id)
        };
        if let Some((_, tx)) = self.waiters.remove(&request_id) {
            let _ = tx.send(outcome.unwrap_or(LookupOutcome::NotFound));
        }
    }

    /// Drop a lookup's state without posting an outcome.
    async fn cleanup(&self, request_id: u64) {
        self.service.lock().await.abandon_lookup(request_id);
    }

    /// Records held locally.
    pub async fn record_count(&self) -> usize {
        self.service.lock().await.record_count()
    }

    /// Keyspace peers known and addressable.
    pub async fn peer_count(&self) -> usize {
        self.service.lock().await.addressable_peers()
    }

    /// Lookups currently awaiting an answer.
    pub fn pending_lookups(&self) -> usize {
        self.waiters.len()
    }

    /// Current metrics.
    pub async fn metrics(&self) -> ResolverMetrics {
        *self.metrics.lock().await
    }

    /// Drop expired records.
    pub async fn sweep(&self, now: u64) -> usize {
        self.service.lock().await.sweep(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::SigningIdentity;
    use crate::mesh::MeshConfig;
    use ed25519_dalek::SigningKey;
    use spine_name::Endpoint;
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;

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

    /// An in-process mesh: envelopes are queued per recipient and pumped by the
    /// test, so delivery order is deterministic and no sockets are involved.
    #[derive(Default)]
    struct Switchboard {
        queues: StdMutex<HashMap<AgentId, Vec<MeshEnvelope>>>,
        /// Agents that should fail to receive, for exercising unreachability.
        blackholed: StdMutex<Vec<AgentId>>,
        /// Symbolic address -> whoever is listening there. Stands in for the
        /// one thing a real socket knows and an agent id does not: that
        /// *something* answers at an address, before you know what.
        listeners: StdMutex<HashMap<String, AgentId>>,
    }

    impl Switchboard {
        fn drain(&self, agent: &AgentId) -> Vec<MeshEnvelope> {
            self.queues
                .lock()
                .unwrap()
                .get_mut(agent)
                .map(std::mem::take)
                .unwrap_or_default()
        }

        fn blackhole(&self, agent: AgentId) {
            self.blackholed.lock().unwrap().push(agent);
        }

        fn listen(&self, endpoint: &str, agent: AgentId) {
            self.listeners
                .lock()
                .unwrap()
                .insert(endpoint.to_string(), agent);
        }

        fn is_empty(&self) -> bool {
            self.queues.lock().unwrap().values().all(|q| q.is_empty())
        }
    }

    struct SwitchTransport {
        board: Arc<Switchboard>,
    }

    #[async_trait]
    impl NameTransport for SwitchTransport {
        async fn send(&self, envelope: MeshEnvelope) -> Result<(), NameMeshError> {
            let target = match envelope.to {
                crate::mesh::MeshTarget::Agent(id) => id,
                // Broadcasts are announcements; the tests deliver them explicitly.
                _ => return Ok(()),
            };
            if self.board.blackholed.lock().unwrap().contains(&target) {
                return Err(NameMeshError::Transport("unreachable".into()));
            }
            self.board
                .queues
                .lock()
                .unwrap()
                .entry(target)
                .or_default()
                .push(envelope);
            Ok(())
        }

        /// Deliver to whoever is listening at `endpoint`, ignoring the
        /// envelope's own target — which is a broadcast, because the sender does
        /// not yet know who it is talking to.
        async fn send_to(
            &self,
            endpoint: &str,
            envelope: MeshEnvelope,
        ) -> Result<(), NameMeshError> {
            let target = self
                .board
                .listeners
                .lock()
                .unwrap()
                .get(endpoint)
                .copied()
                .ok_or_else(|| {
                    NameMeshError::Transport(format!("nothing listening at {endpoint}"))
                })?;
            self.board
                .queues
                .lock()
                .unwrap()
                .entry(target)
                .or_default()
                .push(envelope);
            Ok(())
        }
    }

    struct Harness {
        board: Arc<Switchboard>,
        nodes: Vec<Arc<MeshNameResolver>>,
        identities: Vec<PublicIdentity>,
    }

    impl Harness {
        fn new(count: u8) -> Self {
            let board = Arc::new(Switchboard::default());
            let mut nodes = Vec::new();
            let mut identities = Vec::new();

            for i in 0..count {
                let identity = SigningIdentity::from_seed(&format!("node{i}"), [100 + i; 32]);
                identities.push(identity.public_identity());
                let mesh = Arc::new(MeshNode::new(identity, MeshConfig::default()));
                let transport = Arc::new(SwitchTransport {
                    board: board.clone(),
                });
                nodes.push(Arc::new(
                    MeshNameResolver::new(mesh, transport).with_clock(|| NOW),
                ));
            }
            Self {
                board,
                nodes,
                identities,
            }
        }

        /// Introduce `b` to `a` in both routing spaces.
        async fn introduce(&self, a: usize, b: usize) {
            self.nodes[a]
                .register_peer(&self.identities[b], vec![format!("node{b}:9440")], NOW)
                .await;
        }

        /// Deliver all queued envelopes until the network goes quiet.
        async fn pump(&self) {
            for _ in 0..64 {
                if self.board.is_empty() {
                    return;
                }
                for (i, node) in self.nodes.iter().enumerate() {
                    let agent = *node.node.agent_id();
                    for envelope in self.board.drain(&agent) {
                        let _ = self.nodes[i].handle_envelope(&envelope, NOW).await;
                    }
                }
            }
        }

        /// Keep pumping while another task drives a request, so a round trip can
        /// complete without the test having to interleave the two by hand.
        async fn pump_until(&self, rounds: usize) {
            for _ in 0..rounds {
                self.pump().await;
                tokio::task::yield_now().await;
            }
        }

        /// Put node `i` on the air at a symbolic address.
        fn listen(&self, i: usize, endpoint: &str) {
            self.board.listen(endpoint, *self.nodes[i].node.agent_id());
        }
    }

    /// The property bootstrap exists for: a node holding nothing but an address
    /// ends up holding a verified identity and a usable routing entry.
    #[tokio::test]
    async fn a_node_enters_the_dht_knowing_only_an_address() {
        let h = Harness::new(2);
        h.listen(1, "seed:9440");
        h.nodes[1].set_endpoints(vec!["seed:9440".into()]).await;

        assert_eq!(h.nodes[0].peer_count().await, 0, "no contacts to start");

        let newcomer = h.nodes[0].clone();
        let run = tokio::spawn(async move { newcomer.bootstrap(&["seed:9440".to_string()]).await });
        h.pump_until(8).await;
        let report = run.await.unwrap();

        assert!(report.is_connected(), "the seed answered: {report:?}");
        assert_eq!(report.reached.len(), 1);
        assert_eq!(report.reached[0].agent_id, *h.nodes[1].node.agent_id());
        assert_eq!(
            report.reached[0].key,
            h.nodes[1].local_key(),
            "the identity proved must be the seed's keyspace position"
        );
        assert_eq!(
            h.nodes[0].peer_count().await,
            1,
            "the seed is now an addressable contact"
        );
    }

    /// The seed learns the newcomer too. Bootstrap is not a one-way read: a node
    /// that only pulled contacts would never itself become findable.
    #[tokio::test]
    async fn greeting_a_seed_makes_the_newcomer_known_to_it() {
        let h = Harness::new(2);
        h.listen(1, "seed:9440");
        h.nodes[0].set_endpoints(vec!["newcomer:9440".into()]).await;

        let newcomer = h.nodes[0].clone();
        let run = tokio::spawn(async move { newcomer.bootstrap(&["seed:9440".to_string()]).await });
        h.pump_until(8).await;
        run.await.unwrap();

        assert_eq!(h.nodes[1].peer_count().await, 1);
        assert_eq!(
            h.nodes[1]
                .service
                .lock()
                .await
                .agent_for(&h.nodes[0].local_key()),
            Some(*h.nodes[0].node.agent_id()),
            "the seed must be able to dial the newcomer back"
        );
    }

    /// One seed is meant to be worth a whole routing table: the ack carries the
    /// seed's own contacts, so a single dial yields more than a single peer.
    #[tokio::test]
    async fn a_seed_hands_back_the_peers_it_knows() {
        let h = Harness::new(3);
        h.listen(1, "seed:9440");
        // The seed already knows node 2.
        h.introduce(1, 2).await;

        let newcomer = h.nodes[0].clone();
        let run = tokio::spawn(async move { newcomer.bootstrap(&["seed:9440".to_string()]).await });
        h.pump_until(8).await;
        let report = run.await.unwrap();

        assert_eq!(report.reached[0].referrals, 1, "the seed referred its peer");
        assert_eq!(
            h.nodes[0].peer_count().await,
            2,
            "one dial yielded the seed and its contact"
        );
        assert_eq!(
            h.nodes[0]
                .service
                .lock()
                .await
                .agent_for(&h.nodes[2].local_key()),
            Some(*h.nodes[2].node.agent_id()),
            "a referred peer must arrive dialable, not merely positioned"
        );
    }

    /// A dead seed must cost one failed dial, not a refusal to start. Operators
    /// list spares precisely so one being down is survivable.
    #[tokio::test]
    async fn a_dead_seed_does_not_prevent_bootstrapping_from_a_live_one() {
        let h = Harness::new(2);
        h.listen(1, "live:9440");

        let newcomer = h.nodes[0].clone();
        let seeds = vec!["dead:9440".to_string(), "live:9440".to_string()];
        let run = tokio::spawn(async move { newcomer.bootstrap(&seeds).await });
        h.pump_until(8).await;
        let report = run.await.unwrap();

        assert!(report.is_connected());
        assert_eq!(report.reached.len(), 1);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].0, "dead:9440");
    }

    /// With no seed reachable the node has to say so rather than appear joined.
    #[tokio::test]
    async fn bootstrapping_with_no_reachable_seed_reports_failure() {
        let h = Harness::new(1);
        let report = h.nodes[0].bootstrap(&["nowhere:9440".to_string()]).await;

        assert!(!report.is_connected());
        assert_eq!(report.peers_after, 0);
        assert_eq!(report.failed.len(), 1);
    }

    /// The hello's key is self-asserted, so the signature over it is the only
    /// thing standing between the keyspace and an impostor claiming any
    /// position it likes.
    #[tokio::test]
    async fn a_hello_whose_signature_does_not_match_its_key_is_refused() {
        let h = Harness::new(2);

        let mut hello = h.nodes[0].node.name_hello(vec!["liar:9440".into()]);
        // Claim someone else's keyspace position while keeping our signature.
        if let MeshPayload::NameHello(inner) = &mut hello.payload {
            inner.public_key = *h.nodes[1].local_key().as_bytes();
        }

        assert!(h.nodes[1].handle_envelope(&hello, NOW).await);
        assert_eq!(h.nodes[1].peer_count().await, 0, "no contact was recorded");
        assert_eq!(h.nodes[1].metrics().await.greetings_rejected, 1);
        assert!(h.board.is_empty(), "an impostor gets no answer");
    }

    /// Bootstrap with an empty seed list is a no-op, not an error: a node with
    /// no seeds configured simply is not joining anything.
    #[tokio::test]
    async fn bootstrapping_with_no_seeds_does_nothing() {
        let h = Harness::new(1);
        let report = h.nodes[0].bootstrap(&[]).await;
        assert!(!report.is_connected());
        assert!(report.failed.is_empty());
    }

    #[tokio::test]
    async fn a_locally_held_name_resolves_without_touching_the_wire() {
        let h = Harness::new(1);
        let rec = record(1, &[]);
        h.nodes[0].publish_local(rec.clone()).await.unwrap();

        let found = h.nodes[0].resolve(&rec.name).await.unwrap();
        assert_eq!(found, rec);
        assert!(h.board.is_empty(), "no envelopes should have been sent");
    }

    #[tokio::test]
    async fn registering_a_peer_populates_both_routing_spaces() {
        let h = Harness::new(2);
        h.introduce(0, 1).await;

        assert_eq!(h.nodes[0].peer_count().await, 1);
        // The keyspace id is the peer's signing key, so the mapping is exact.
        let peer_key = h.nodes[1].local_key();
        assert_eq!(
            h.nodes[0].service.lock().await.agent_for(&peer_key),
            Some(*h.nodes[1].node.agent_id())
        );
    }

    #[tokio::test]
    async fn a_peer_whose_key_is_not_ed25519_is_skipped_not_guessed_at() {
        let h = Harness::new(1);
        let mut bogus = h.identities[0].clone();
        bogus.public_key = vec![1, 2, 3]; // wrong length
        h.nodes[0].register_peer(&bogus, vec![], NOW).await;
        assert_eq!(h.nodes[0].peer_count().await, 0);
    }

    #[tokio::test]
    async fn a_name_resolves_across_one_hop() {
        let h = Harness::new(2);
        h.introduce(0, 1).await;

        let rec = record(1, &[]);
        h.nodes[1].publish_local(rec.clone()).await.unwrap();

        // Drive the lookup and the pump concurrently: resolve() awaits an answer
        // that only arrives once the switchboard is drained.
        let seeker = h.nodes[0].clone();
        let name = rec.name.clone();
        let lookup = tokio::spawn(async move { seeker.resolve(&name).await });

        for _ in 0..16 {
            h.pump().await;
            tokio::task::yield_now().await;
        }

        let found = lookup.await.unwrap().unwrap();
        assert_eq!(found, rec, "resolved a name held by another node");
        assert!(found.verify().is_ok());
    }

    #[tokio::test]
    async fn a_capability_resolves_across_the_mesh() {
        let h = Harness::new(3);
        h.introduce(0, 1).await;
        h.introduce(0, 2).await;

        h.nodes[1].publish_local(record(1, &["web.search"])).await.unwrap();
        h.nodes[2].publish_local(record(2, &["web.search"])).await.unwrap();

        let seeker = h.nodes[0].clone();
        let lookup = tokio::spawn(async move { seeker.find_providers("web.search").await });

        for _ in 0..16 {
            h.pump().await;
            tokio::task::yield_now().await;
        }

        let providers = lookup.await.unwrap().unwrap();
        assert_eq!(providers.len(), 2, "collected providers from both holders");
        assert!(providers.iter().all(|p| p.verify().is_ok()));
    }

    #[tokio::test]
    async fn an_announcement_propagates_a_verified_record() {
        let h = Harness::new(2);
        let rec = record(1, &["web.search"]);

        let envelope = h.nodes[0].node.announce_name(rec.clone()).unwrap();
        assert!(h.nodes[1].handle_envelope(&envelope, NOW).await);

        assert_eq!(h.nodes[1].record_count().await, 1);
        assert_eq!(h.nodes[1].metrics().await.announcements_accepted, 1);
    }

    #[tokio::test]
    async fn a_forged_announcement_is_rejected_at_the_receiver() {
        let h = Harness::new(2);
        let mut forged = record(1, &[]);
        forged.capabilities.push("admin".into()); // breaks the signature

        // Construct the envelope directly: announce_name would refuse to build it.
        let envelope = h.nodes[0].node.create_envelope(
            crate::mesh::MeshTarget::Broadcast,
            MeshPayload::NameAnnounce(Box::new(crate::naming::AnnouncedRecord::new(forged))),
        );
        assert!(h.nodes[1].handle_envelope(&envelope, NOW).await);

        assert_eq!(h.nodes[1].record_count().await, 0, "nothing forged is stored");
        assert_eq!(h.nodes[1].metrics().await.announcements_rejected, 1);
    }

    #[tokio::test]
    async fn a_request_is_answered_over_the_wire() {
        let h = Harness::new(2);
        let rec = record(1, &[]);
        h.nodes[1].publish_local(rec.clone()).await.unwrap();

        let envelope = h.nodes[0].node.name_resolve_request(
            *h.nodes[1].node.agent_id(),
            77,
            ResolveQuery::Name(rec.name.clone()),
        );
        assert!(h.nodes[1].handle_envelope(&envelope, NOW).await);

        // The answer is queued back to the asker.
        let replies = h.board.drain(h.nodes[0].node.agent_id());
        assert_eq!(replies.len(), 1);
        match &replies[0].payload {
            MeshPayload::NameResolveResponse(r) => {
                assert_eq!(r.request_id, 77);
                assert_eq!(r.record.as_ref(), Some(&rec));
            }
            other => panic!("expected a response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_non_naming_envelope_is_left_for_the_caller() {
        let h = Harness::new(1);
        let envelope = h.nodes[0]
            .node
            .create_envelope(crate::mesh::MeshTarget::Broadcast, MeshPayload::Ping(1));
        assert!(
            !h.nodes[0].handle_envelope(&envelope, NOW).await,
            "the driver must not swallow unrelated mesh traffic"
        );
    }

    #[tokio::test]
    async fn a_lookup_with_no_reachable_peers_settles_instead_of_hanging() {
        let h = Harness::new(2);
        h.introduce(0, 1).await;
        h.board.blackhole(*h.nodes[1].node.agent_id());

        // Must return promptly with NotFound, not wait out the timeout.
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            h.nodes[0].resolve(&record(1, &[]).name),
        )
        .await
        .expect("resolve() hung despite having no reachable peer");

        assert!(matches!(result, Err(NameMeshError::NotFound(_))));
        assert!(h.nodes[0].metrics().await.unroutable_peers >= 1);
        assert_eq!(h.nodes[0].pending_lookups(), 0, "no waiter left behind");
    }

    #[tokio::test]
    async fn a_silent_peer_times_the_lookup_out_and_cleans_up() {
        let h = Harness::new(2);
        h.introduce(0, 1).await;

        // The request is delivered to the switchboard but never pumped, so the
        // peer simply never answers.
        let resolver = MeshNameResolver::new(
            h.nodes[0].node.clone(),
            Arc::new(SwitchTransport {
                board: h.board.clone(),
            }),
        )
        .with_timeout(Duration::from_millis(150))
        .with_clock(|| NOW);
        resolver
            .register_peer(&h.identities[1], vec!["node1:9440".into()], NOW)
            .await;

        let err = resolver.resolve(&record(1, &[]).name).await.unwrap_err();
        assert!(matches!(err, NameMeshError::Timeout(_)));
        assert_eq!(resolver.pending_lookups(), 0, "the waiter was cleaned up");
        assert_eq!(resolver.metrics().await.lookups_timed_out, 1);
    }

    #[tokio::test]
    async fn publishing_announces_to_the_mesh_and_stores_locally() {
        let h = Harness::new(1);
        let rec = record(1, &["web.search"]);
        h.nodes[0].publish(rec.clone()).await.unwrap();
        assert_eq!(h.nodes[0].record_count().await, 1);
    }

    #[tokio::test]
    async fn publishing_a_forged_record_fails_before_it_reaches_the_wire() {
        let h = Harness::new(1);
        let mut forged = record(1, &[]);
        forged.seq += 1; // breaks the signature

        assert!(h.nodes[0].publish(forged).await.is_err());
        assert_eq!(h.nodes[0].record_count().await, 0);
        assert!(h.board.is_empty());
    }

    /// A record whose key sits closer to `near` than to `far`.
    ///
    /// Found by search rather than constructed: a record's key is the hash of a
    /// name nobody chooses for its position, so which node a given record
    /// belongs to is a fact about the keys, not something a test can arrange.
    fn record_nearer(near: NameKey, far: NameKey) -> NameRecord {
        (1u8..=255)
            .map(|seed| record(seed, &[]))
            .find(|rec| {
                let target = rec.name.key();
                near.distance(&target) < far.distance(&target)
            })
            .expect("some seed lands nearer")
    }

    /// The point of replication: after publishing, the record exists somewhere
    /// other than the publisher.
    #[tokio::test]
    async fn publishing_places_copies_at_the_closest_nodes() {
        let h = Harness::new(3);
        h.introduce(0, 1).await;
        h.introduce(0, 2).await;

        let publisher = h.nodes[0].clone();
        let rec = record(1, &["web.search"]);
        let run = tokio::spawn(async move { publisher.publish(rec).await.unwrap() });
        h.pump_until(16).await;
        let report = run.await.unwrap();

        assert!(report.is_durable(), "nothing was replicated: {report:?}");
        assert_eq!(report.sent, 2, "both keyspace peers should hold a copy");
        assert!(!report.broadcast, "a directed store is not a broadcast");
        assert_eq!(h.nodes[1].record_count().await, 1);
        assert_eq!(h.nodes[2].record_count().await, 1);
    }

    /// The stronger property: a copy reaches a node the publisher was never
    /// introduced to, because the walk found it. Without the keyspace walk,
    /// replication would only ever reach nodes already in the routing table —
    /// which are not the nodes a lookup for that record will ask.
    #[tokio::test]
    async fn a_replica_reaches_a_node_the_publisher_never_met() {
        let h = Harness::new(3);
        h.introduce(0, 1).await;
        h.introduce(1, 2).await;

        // Node 2 must be the right home for the record, or node 1 has no reason
        // to refer it — a peer only refers nodes closer to the target than
        // itself.
        let rec = record_nearer(h.nodes[2].local_key(), h.nodes[1].local_key());
        assert_eq!(h.nodes[0].peer_count().await, 1, "node 2 is a stranger");

        let publisher = h.nodes[0].clone();
        let handed = rec.clone();
        let run = tokio::spawn(async move { publisher.publish(handed).await.unwrap() });
        h.pump_until(16).await;
        let report = run.await.unwrap();

        assert_eq!(
            h.nodes[2].record_count().await,
            1,
            "the record should have travelled to where it belongs: {report:?}"
        );
    }

    /// A node with no keyspace peers has nowhere durable to put a record, and
    /// says so rather than letting a broadcast pass for a replica.
    #[tokio::test]
    async fn a_publisher_alone_in_the_mesh_reports_nothing_durable() {
        let h = Harness::new(1);
        let report = h.nodes[0].publish(record(1, &[])).await.unwrap();

        assert!(!report.is_durable());
        assert_eq!(report.replicas(), 0);
        assert!(report.broadcast, "it still warms whatever caches are listening");
        assert_eq!(h.nodes[0].record_count().await, 1, "held locally regardless");
    }

    /// Churn insurance. A record published while the node was alone belongs on
    /// the peer that showed up afterwards, and maintenance is what puts it there.
    #[tokio::test]
    async fn maintenance_re_offers_a_record_to_a_peer_that_arrived_later() {
        let h = Harness::new(2);
        h.nodes[0].publish(record(1, &[])).await.unwrap();
        assert_eq!(h.nodes[1].record_count().await, 0);

        h.introduce(0, 1).await;

        let node = h.nodes[0].clone();
        let run = tokio::spawn(async move { node.maintain(NOW, MaintenancePolicy::default()).await });
        h.pump_until(16).await;
        let report = run.await.unwrap();

        assert_eq!(report.refreshed, 1);
        assert_eq!(report.replicas_sent, 1);
        assert_eq!(
            h.nodes[1].record_count().await,
            1,
            "the late arrival now holds the record"
        );
    }

    #[tokio::test]
    async fn maintenance_drops_a_record_whose_ttl_ran_out() {
        let h = Harness::new(1);
        h.nodes[0].publish(record(1, &[])).await.unwrap();

        let report = h.nodes[0].maintain(NOW + 7200, MaintenancePolicy::default()).await;
        assert_eq!(report.expired, 1);
        assert_eq!(h.nodes[0].record_count().await, 0);
        assert_eq!(report.refreshed, 0, "nothing left to refresh");
    }

    /// A bounded pass must not re-offer the same prefix forever. Each pass
    /// resumes after the last key the previous one handled, so a store larger
    /// than the budget is covered across ticks rather than having its tail
    /// starved — and it says how many it left, so a bounded pass never reads as
    /// an exhaustive one.
    #[tokio::test]
    async fn a_bounded_pass_resumes_where_the_last_one_stopped() {
        let h = Harness::new(2);
        for seed in 1..=4u8 {
            h.nodes[0].publish(record(seed, &[])).await.unwrap();
        }
        h.introduce(0, 1).await;

        let policy = MaintenancePolicy {
            lapse_window_secs: 0,
            max_records: 2,
        };

        let node = h.nodes[0].clone();
        let run = tokio::spawn(async move { node.maintain(NOW, policy).await });
        h.pump_until(24).await;
        let first = run.await.unwrap();

        assert_eq!(first.refreshed, 2, "the budget is two: {first:?}");
        assert_eq!(first.deferred, 2, "and it must say it left two");

        let node = h.nodes[0].clone();
        let run = tokio::spawn(async move { node.maintain(NOW, policy).await });
        h.pump_until(24).await;
        let second = run.await.unwrap();

        assert_eq!(second.refreshed, 2);
        // All four are now on the peer, which only happens if the second pass
        // handled the two the first one skipped.
        assert_eq!(
            h.nodes[1].record_count().await,
            4,
            "two passes of two must cover four records, not repeat the first two"
        );
    }

    /// Maintenance cannot renew a name — the expiry is signed into the record —
    /// so a name about to lapse comes back as a report for its owner.
    #[tokio::test]
    async fn a_lapsing_name_is_reported_rather_than_renewed() {
        let h = Harness::new(1);
        let rec = record(1, &[]);
        h.nodes[0].publish(rec.clone()).await.unwrap();

        // 600 seconds of a 3600-second TTL left, against a 900-second window.
        let report = h.nodes[0]
            .maintain(
                NOW + 3000,
                MaintenancePolicy {
                    lapse_window_secs: 900,
                    ..Default::default()
                },
            )
            .await;
        assert_eq!(report.lapsing, vec![rec.name.clone()]);
        assert_eq!(report.expired, 0);
        assert_eq!(
            h.nodes[0].record_count().await,
            1,
            "still valid, just not for long"
        );
    }
}
