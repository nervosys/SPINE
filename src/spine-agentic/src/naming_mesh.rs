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
use crate::naming::{LookupOutcome, NameService, ResolveQuery, ResolveResponse};
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
}

/// A [`NameService`] wired to a live mesh.
pub struct MeshNameResolver {
    service: Mutex<NameService>,
    node: Arc<MeshNode>,
    transport: Arc<dyn NameTransport>,
    /// Keyspace position -> mesh identity. The bridge between the two spaces.
    peer_index: DashMap<NameKey, AgentId>,
    /// Lookups awaiting an outcome, by request id.
    waiters: DashMap<u64, oneshot::Sender<LookupOutcome>>,
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
            .field("known_peers", &self.peer_index.len())
            .field("pending_lookups", &self.waiters.len())
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
            peer_index: DashMap::new(),
            waiters: DashMap::new(),
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
        self.peer_index.insert(key, identity.agent_id);
        self.service
            .lock()
            .await
            .add_node(NodeInfo::new(key, endpoints, now));
    }

    /// Publish a signed record into the local store and announce it to the mesh.
    pub async fn publish(&self, record: NameRecord) -> Result<(), NameMeshError> {
        self.service.lock().await.publish(record.clone())?;
        // announce_name re-verifies, so an invalid record never reaches the wire.
        let envelope = self.node.announce_name(record)?;
        self.transport.send(envelope).await
    }

    /// Publish locally without announcing — for names that should be resolvable
    /// but not advertised.
    pub async fn publish_local(&self, record: NameRecord) -> Result<(), NameMeshError> {
        self.service.lock().await.publish(record)?;
        Ok(())
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
        let query = {
            let service = self.service.lock().await;
            match service.lookup_query(request_id) {
                Some(q) => q,
                None => return,
            }
        };

        let mut unreachable = Vec::new();
        let mut sends = Vec::new();

        for node in wave {
            // A keyspace neighbour we cannot map to a mesh identity is not
            // addressable. Treat it as a timeout so the lookup stops waiting on
            // it rather than stalling.
            let Some(agent_id) = self.peer_index.get(&node.id).map(|e| *e.value()) else {
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
                // publish() verifies, so a forged announcement is counted and
                // dropped rather than becoming servable state.
                let accepted = self
                    .service
                    .lock()
                    .await
                    .publish(announced.record.clone())
                    .is_ok();
                let mut m = self.metrics.lock().await;
                if accepted {
                    m.announcements_accepted += 1;
                } else {
                    m.announcements_rejected += 1;
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

            _ => false,
        }
    }

    /// Feed a response into its lookup and continue or finish it.
    async fn absorb_response(&self, response: &ResolveResponse) {
        let request_id = response.request_id;
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

    /// Keyspace peers known.
    pub async fn peer_count(&self) -> usize {
        self.peer_index.len()
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
            h.nodes[0].peer_index.get(&peer_key).map(|e| *e.value()),
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
}
