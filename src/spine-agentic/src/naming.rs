//! Mesh-backed name resolution — the piece that makes the namespace *federated*
//! rather than a per-process directory.
//!
//! The mesh already gossiped peers, routed multi-hop, and deduplicated by
//! message id. What it could not do was find something it did not already know
//! the address of. [`spine_name`] supplies the keyspace; this module drives a
//! Kademlia iterative lookup across it using the mesh as transport.
//!
//! ## How a lookup terminates
//!
//! [`Lookup`] is a state machine, not a loop that owns a socket. The caller
//! pumps it: it hands out the next wave of nodes to query, absorbs whatever
//! comes back, and reports when it has converged. Structuring it this way keeps
//! the convergence argument testable without a network, and lets the same logic
//! run over TCP, WebSocket, QUIC, or an in-process harness unchanged.
//!
//! Termination is guaranteed by two independent bounds: a node is queried at
//! most once (`queried` is monotonic), and the round counter is capped. The
//! usual Kademlia argument — each round strictly halves the distance — gives
//! O(log n) rounds in the common case; the cap covers the adversarial one, where
//! a malicious peer returns nodes that never get closer.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use spine_name::{NameKey, NameRecord, NodeInfo, RoutingTable, SpineUri};

use crate::AgentId;

/// How many peers a lookup queries per round. Kademlia's α: enough parallelism
/// to hide one slow peer, small enough not to flood the mesh.
pub const ALPHA: usize = 3;

/// How many closest nodes a lookup tracks, and how many a node returns.
pub const K: usize = 20;

/// Hard cap on lookup rounds. Only reached when peers misbehave; the honest
/// case converges in O(log n).
pub const MAX_ROUNDS: usize = 16;

/// A signed record as it travels the mesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnouncedRecord {
    pub record: NameRecord,
}

impl AnnouncedRecord {
    pub fn new(record: NameRecord) -> Self {
        Self { record }
    }
}

/// What a resolution request is asking for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolveQuery {
    /// Resolve one name to its record.
    Name(SpineUri),
    /// Find providers of a capability term.
    Capability(String),
    /// Find the nodes closest to a keyspace point, with no record to fetch.
    ///
    /// Kademlia's FIND_NODE, and storing a record is what needs it: to put a
    /// record at the K closest nodes you first have to know which nodes those
    /// are, and a name lookup cannot tell you. A name lookup stops the moment
    /// any node hands back the record — usually long before the walk has
    /// converged on the neighbourhood the record belongs in.
    Node(NameKey),
}

impl ResolveQuery {
    /// The keyspace point this query routes toward.
    ///
    /// Capability queries hash the term itself, so providers of `web.search`
    /// cluster near one point and a lookup converges on them the same way it
    /// converges on a name. This is what makes "who can do X" a routed question
    /// instead of a broadcast.
    pub fn target_key(&self) -> NameKey {
        match self {
            ResolveQuery::Name(uri) => uri.key(),
            ResolveQuery::Capability(term) => {
                NameKey::of(format!("cap:{}", term.to_ascii_lowercase()).as_bytes())
            }
            ResolveQuery::Node(key) => *key,
        }
    }
}

/// A request for resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveRequest {
    /// Correlates the response. The mesh is fire-and-forget, so a reply has to
    /// carry the id that identifies which question it answers.
    pub request_id: u64,
    pub query: ResolveQuery,
}

/// A keyspace neighbour together with the mesh identity needed to address it.
///
/// The two routing spaces have to travel together. A bare [`NodeInfo`] says
/// *where a node sits in the keyspace* and what addresses it claims, but an
/// envelope is addressed to an [`AgentId`] — so a peer learned purely as a
/// `NodeInfo` is a peer nothing can send to. Referrals that omit the agent id
/// therefore look like progress and produce none: the lookup adds the node to
/// its shortlist, fails to dispatch to it, and marks it unreachable.
///
/// Pairing them at the point of referral is what lets a lookup walk to nodes it
/// was never manually introduced to, which is the whole point of a DHT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyspacePeer {
    pub info: NodeInfo,
    pub agent_id: AgentId,
}

impl KeyspacePeer {
    pub fn new(info: NodeInfo, agent_id: AgentId) -> Self {
        Self { info, agent_id }
    }

    /// Keyspace position — shorthand for `self.info.id`.
    pub fn key(&self) -> NameKey {
        self.info.id
    }
}

/// An answer to a [`ResolveRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveResponse {
    pub request_id: u64,
    /// The exact record, if this node holds it.
    pub record: Option<NameRecord>,
    /// Providers, for a capability query.
    #[serde(default)]
    pub providers: Vec<NameRecord>,
    /// Nodes closer to the target than this one — how a lookup makes progress
    /// when the answer is elsewhere. Carries mesh identities as well as keyspace
    /// positions, so the asker can actually dial what it is referred to.
    #[serde(default)]
    pub closer: Vec<KeyspacePeer>,
}

impl ResolveResponse {
    pub fn empty(request_id: u64) -> Self {
        Self {
            request_id,
            record: None,
            providers: Vec::new(),
            closer: Vec::new(),
        }
    }

    /// Whether this response answers the question outright.
    pub fn is_answer(&self) -> bool {
        self.record.is_some() || !self.providers.is_empty()
    }
}

/// A node introducing itself to a peer it reached by address alone.
///
/// Bootstrap's chicken-and-egg problem is that every other message is addressed
/// to an [`AgentId`], but a seed node is known only as `host:port`. So the hello
/// carries the sender's Ed25519 key explicitly rather than relying on the
/// recipient already holding it: the envelope signature verifies *against the
/// carried key*, which proves the sender holds the corresponding private key and
/// binds that key to the envelope's `from` agent id.
///
/// That is not a claim taken on trust. The key is simultaneously the sender's
/// keyspace position, so it cannot lie about where in the DHT it sits without
/// producing a signature it cannot make — the same self-certification a `did:`
/// name has, applied to node identity. What it *can* lie about is which
/// endpoints it claims, which costs a failed dial and nothing more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameHello {
    /// The sender's Ed25519 public key, and so its keyspace position.
    pub public_key: [u8; 32],
    /// Addresses at which the sender can be reached.
    #[serde(default)]
    pub endpoints: Vec<String>,
}

/// The answer to a [`NameHello`]: who answered, and who else to talk to.
///
/// Returning neighbours here rather than making the newcomer run a separate
/// query is what turns one reachable seed into a populated routing table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameHelloAck {
    /// The responder's Ed25519 public key, verified the same way as [`NameHello`].
    pub public_key: [u8; 32],
    /// Addresses at which the responder can be reached.
    #[serde(default)]
    pub endpoints: Vec<String>,
    /// Peers the responder knows, to seed the newcomer's routing table.
    #[serde(default)]
    pub closer: Vec<KeyspacePeer>,
}

/// The outcome of a completed lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupOutcome {
    /// The name resolved.
    Found(Box<NameRecord>),
    /// A capability query found providers.
    Providers(Vec<NameRecord>),
    /// A node query converged on these peers, nearest-first.
    Closest(Vec<NodeInfo>),
    /// The keyspace was exhausted without an answer.
    NotFound,
}

/// An in-flight iterative lookup.
///
/// Pump it: [`Lookup::next_wave`] yields nodes to query, [`Lookup::on_response`]
/// absorbs what they say, [`Lookup::outcome`] reports when it is done.
#[derive(Debug)]
pub struct Lookup {
    query: ResolveQuery,
    target: NameKey,
    /// Candidate nodes, nearest-first, capped at K.
    shortlist: Vec<NodeInfo>,
    /// Nodes already sent a request — the monotonic set that bounds the walk.
    queried: HashSet<NameKey>,
    /// Best answer seen so far.
    record: Option<NameRecord>,
    providers: Vec<NameRecord>,
    rounds: usize,
    /// Requests sent but not yet answered or timed out.
    ///
    /// Without this a lookup would declare itself exhausted the moment its
    /// shortlist ran dry, discarding answers from peers still in flight — which
    /// silently truncates a capability query to whatever its fastest peer knew.
    in_flight: usize,
    /// Set once no unqueried candidate remains, nothing is in flight, or the
    /// round cap is hit.
    exhausted: bool,
}

impl Lookup {
    /// Start a lookup seeded with the closest nodes already known locally.
    pub fn new(query: ResolveQuery, seeds: Vec<NodeInfo>) -> Self {
        let target = query.target_key();
        let mut lookup = Self {
            query,
            target,
            shortlist: Vec::new(),
            queried: HashSet::new(),
            record: None,
            providers: Vec::new(),
            rounds: 0,
            in_flight: 0,
            exhausted: false,
        };
        lookup.absorb_nodes(seeds);
        lookup
    }

    /// The keyspace point being sought.
    pub fn target(&self) -> &NameKey {
        &self.target
    }

    /// The query being resolved.
    pub fn query(&self) -> &ResolveQuery {
        &self.query
    }

    /// Nodes queried so far.
    pub fn queried_count(&self) -> usize {
        self.queried.len()
    }

    /// Rounds issued so far.
    pub fn rounds(&self) -> usize {
        self.rounds
    }

    /// The next up-to-α nodes to query, nearest-first.
    ///
    /// Returns empty when the lookup is finished — either it has an answer, it
    /// has run out of unqueried candidates, or it has hit [`MAX_ROUNDS`].
    pub fn next_wave(&mut self) -> Vec<NodeInfo> {
        if self.is_done() {
            return Vec::new();
        }
        if self.rounds >= MAX_ROUNDS {
            self.exhausted = true;
            return Vec::new();
        }

        let wave: Vec<NodeInfo> = self
            .shortlist
            .iter()
            .filter(|n| !self.queried.contains(&n.id))
            .take(ALPHA)
            .cloned()
            .collect();

        if wave.is_empty() {
            // Only truly out of options once nothing is still outstanding.
            if self.in_flight == 0 {
                self.exhausted = true;
            }
            return Vec::new();
        }

        for node in &wave {
            self.queried.insert(node.id);
        }
        self.in_flight += wave.len();
        self.rounds += 1;
        wave
    }

    /// Absorb a peer's response.
    pub fn on_response(&mut self, response: &ResolveResponse) {
        self.in_flight = self.in_flight.saturating_sub(1);

        // A record only counts if it verifies and actually answers *this* query.
        // A peer that returns someone else's record — or a forgery — must not be
        // able to redirect the lookup.
        if let Some(record) = &response.record {
            if record.verify().is_ok() && self.matches_query(record) {
                let better = match &self.record {
                    Some(existing) => record.supersedes(existing),
                    None => true,
                };
                if better {
                    self.record = Some(record.clone());
                }
            }
        }

        for provider in &response.providers {
            if provider.verify().is_ok()
                && self.wants_capability(provider)
                && !self.providers.iter().any(|p| p.name == provider.name)
            {
                self.providers.push(provider.clone());
            }
        }

        self.absorb_nodes(response.closer.iter().map(|p| p.info.clone()).collect());
    }

    /// Mark a node as unreachable so the lookup stops waiting on it.
    pub fn on_timeout(&mut self, node: &NameKey) {
        if self.queried.contains(node) {
            self.in_flight = self.in_flight.saturating_sub(1);
        }
        self.queried.insert(*node);
        self.shortlist.retain(|n| &n.id != node);
    }

    /// Requests sent but not yet answered.
    pub fn in_flight(&self) -> usize {
        self.in_flight
    }

    /// Whether the lookup has finished.
    pub fn is_done(&self) -> bool {
        if self.exhausted {
            return true;
        }
        match &self.query {
            // A name resolves to exactly one record; the first valid one ends it.
            ResolveQuery::Name(_) => self.record.is_some(),
            // A capability query wants breadth, so it keeps going until the
            // keyspace is exhausted or it has collected K providers.
            ResolveQuery::Capability(_) => self.providers.len() >= K,
            // A node query has no early answer to stop on. Its result *is* the
            // converged shortlist, so it runs until the keyspace is exhausted.
            ResolveQuery::Node(_) => false,
        }
    }

    /// The result, if the lookup has finished.
    pub fn outcome(&self) -> Option<LookupOutcome> {
        if !self.is_done() {
            return None;
        }
        if matches!(self.query, ResolveQuery::Node(_)) {
            return Some(LookupOutcome::Closest(self.shortlist.clone()));
        }
        if let Some(record) = &self.record {
            return Some(LookupOutcome::Found(Box::new(record.clone())));
        }
        if !self.providers.is_empty() {
            return Some(LookupOutcome::Providers(self.providers.clone()));
        }
        Some(LookupOutcome::NotFound)
    }

    fn matches_query(&self, record: &NameRecord) -> bool {
        match &self.query {
            ResolveQuery::Name(uri) => record.name.key() == uri.key(),
            ResolveQuery::Capability(term) => record.has_capability(term),
            // A node query asks about the keyspace, not about names. Refusing
            // every record keeps a peer from ending the walk early by answering
            // a question that was not asked.
            ResolveQuery::Node(_) => false,
        }
    }

    fn wants_capability(&self, record: &NameRecord) -> bool {
        match &self.query {
            ResolveQuery::Capability(term) => record.has_capability(term),
            ResolveQuery::Name(_) | ResolveQuery::Node(_) => false,
        }
    }

    /// Merge candidates into the shortlist, keeping it sorted and capped.
    fn absorb_nodes(&mut self, nodes: Vec<NodeInfo>) {
        for node in nodes {
            if self.shortlist.iter().any(|n| n.id == node.id) {
                continue;
            }
            self.shortlist.push(node);
        }
        let target = self.target;
        self.shortlist.sort_by_key(|n| n.id.distance(&target));
        self.shortlist.truncate(K);
    }
}

/// A node's naming service: the records it serves, the keyspace peers it knows,
/// and the lookups it has in flight.
#[derive(Debug)]
pub struct NameService {
    /// Where this node sits in the keyspace.
    local_key: NameKey,
    /// Records this node holds and serves.
    store: spine_name::RecordStore,
    /// Keyspace-aware peer table, distinct from the mesh's AgentId routing.
    routing: RoutingTable,
    /// Keyspace position -> mesh identity: the bridge between the two routing
    /// spaces. Kept beside the routing table rather than in the driver above it
    /// because [`NameService::handle_request`] has to *emit* mesh identities
    /// when it refers a peer onward, not merely consume them.
    peers: HashMap<NameKey, AgentId>,
    /// In-flight lookups by request id.
    lookups: HashMap<u64, Lookup>,
    next_request_id: u64,
}

impl NameService {
    /// A service for a node whose keyspace position is `local_key` — for a SPINE
    /// agent, its Ed25519 public key, so a node id is self-certifying exactly
    /// like a `did:` name.
    pub fn new(local_key: NameKey) -> Self {
        Self {
            local_key,
            store: spine_name::RecordStore::new(),
            routing: RoutingTable::new(local_key),
            peers: HashMap::new(),
            lookups: HashMap::new(),
            next_request_id: 1,
        }
    }

    /// This node's keyspace position.
    pub fn local_key(&self) -> &NameKey {
        &self.local_key
    }

    /// Store a signed record. Verification happens inside the store, so an
    /// unverifiable announcement cannot enter.
    ///
    /// The outcome matters to whatever is above this: a record that was
    /// superseded by one already held must not be replicated onward, or two
    /// nodes holding different versions will announce at each other forever.
    pub fn publish(
        &mut self,
        record: NameRecord,
    ) -> Result<spine_name::PutOutcome, spine_name::NameError> {
        self.store.put(record)
    }

    /// Learn about a keyspace peer whose mesh identity is not known.
    ///
    /// Such a peer can be *referred to* but not dialed, so prefer
    /// [`NameService::add_peer`] wherever the agent id is available.
    pub fn add_node(&mut self, node: NodeInfo) -> bool {
        self.routing.insert(node)
    }

    /// Learn about a keyspace peer in both routing spaces at once.
    pub fn add_peer(&mut self, node: NodeInfo, agent_id: AgentId) -> bool {
        self.peers.insert(node.id, agent_id);
        self.routing.insert(node)
    }

    /// The mesh identity of a keyspace peer, if this node knows it.
    pub fn agent_for(&self, key: &NameKey) -> Option<AgentId> {
        self.peers.get(key).copied()
    }

    /// Keyspace peers that can actually be addressed.
    pub fn addressable_peers(&self) -> usize {
        self.peers.len()
    }

    /// Pair each node with its mesh identity, dropping any that has none.
    ///
    /// Referring a peer nobody can dial is worse than staying silent: it looks
    /// like progress, so the asker spends a round of its budget discovering that
    /// the contact is useless.
    fn as_keyspace_peers(&self, nodes: Vec<NodeInfo>) -> Vec<KeyspacePeer> {
        nodes
            .into_iter()
            .filter_map(|n| {
                self.peers
                    .get(&n.id)
                    .map(|agent| KeyspacePeer::new(n, *agent))
            })
            .collect()
    }

    /// Records held locally.
    pub fn record_count(&self) -> usize {
        self.store.len()
    }

    /// Keyspace peers known.
    pub fn node_count(&self) -> usize {
        self.routing.len()
    }

    /// Answer a peer's resolution request from local knowledge.
    ///
    /// Always returns closer nodes alongside any answer: a peer that got a
    /// partial answer should still be able to make progress without a second
    /// round trip.
    pub fn handle_request(&self, request: &ResolveRequest, now: u64) -> ResolveResponse {
        let target = request.query.target_key();
        let mut response = ResolveResponse::empty(request.request_id);

        match &request.query {
            ResolveQuery::Name(uri) => {
                response.record = self.store.get_fresh(uri, now).cloned();
            }
            ResolveQuery::Capability(term) => {
                response.providers = self
                    .store
                    .providers_of(term, now)
                    .into_iter()
                    .take(K)
                    .cloned()
                    .collect();
            }
            // Nothing to look up: the closer peers appended below are the whole
            // answer to a node query.
            ResolveQuery::Node(_) => {}
        }

        let closer: Vec<NodeInfo> = self
            .routing
            .closest(&target, K)
            .into_iter()
            .filter(|n| n.id.distance(&target) < self.local_key.distance(&target))
            .collect();
        response.closer = self.as_keyspace_peers(closer);

        response
    }

    /// Peers to offer a newcomer, nearest to its own keyspace position.
    ///
    /// Answering with the neighbours of *the newcomer's* key rather than a
    /// random sample is what makes a single hello worth a round of Kademlia: the
    /// contacts it gets back are the ones that belong in its own low buckets.
    pub fn peers_for_newcomer(&self, newcomer: &NameKey) -> Vec<KeyspacePeer> {
        let closest = self
            .routing
            .closest(newcomer, K + 1)
            .into_iter()
            // Never refer a node to itself. The newcomer was just added to this
            // table, and it is by definition the closest entry to its own key,
            // so without this it would top every list it asked for — spending a
            // referral slot, and a dispatch, on a peer it already is. Ask for
            // K + 1 so dropping it still leaves K.
            .filter(|n| &n.id != newcomer)
            .take(K)
            .collect();
        self.as_keyspace_peers(closest)
    }

    /// The `n` addressable peers nearest `key`, nearest-first.
    ///
    /// This is where a record's replicas belong. Peers with no known mesh
    /// identity are dropped rather than counted: a replica sent nowhere is not a
    /// replica, and reporting one would overstate how many copies exist.
    pub fn closest_peers(&self, key: &NameKey, n: usize) -> Vec<KeyspacePeer> {
        let closest = self
            .routing
            .closest(key, n + 1)
            .into_iter()
            .filter(|node| node.id != self.local_key)
            .take(n)
            .collect();
        self.as_keyspace_peers(closest)
    }

    /// Records held locally, cheapest-first to iterate for maintenance.
    pub fn records(&self) -> Vec<NameRecord> {
        self.store.records().cloned().collect()
    }

    /// Records within `window` seconds of lapsing.
    ///
    /// Only the holder of the signing key can extend a record's life, so this is
    /// a report rather than something the service can act on: re-announcing a
    /// record does not move its expiry, which is signed into it.
    pub fn lapsing(&self, now: u64, window: u64) -> Vec<SpineUri> {
        self.store
            .needing_republish(now, window)
            .into_iter()
            .map(|r| r.name.clone())
            .collect()
    }

    /// Answer a query from local knowledge alone, if it can be.
    ///
    /// Checked before any lookup starts: a node that already holds the answer
    /// has no business walking the keyspace for it.
    pub fn resolve_locally(&self, query: &ResolveQuery, now: u64) -> Option<LookupOutcome> {
        match query {
            ResolveQuery::Name(uri) => self
                .store
                .get_fresh(uri, now)
                .map(|r| LookupOutcome::Found(Box::new(r.clone()))),
            ResolveQuery::Capability(term) => {
                let providers: Vec<NameRecord> = self
                    .store
                    .providers_of(term, now)
                    .into_iter()
                    .take(K)
                    .cloned()
                    .collect();
                // An empty local index is not an answer — the mesh may know more.
                (!providers.is_empty()).then_some(LookupOutcome::Providers(providers))
            }
            // Never answerable locally. This node's own view of the
            // neighbourhood is precisely what a node query exists to correct:
            // the peers that joined nearest a key since the last walk are the
            // ones missing from the routing table, and they are the ones a
            // replica has to reach.
            ResolveQuery::Node(_) => None,
        }
    }

    /// Begin a lookup, returning its request id and the first wave to query.
    pub fn start_lookup(&mut self, query: ResolveQuery) -> (u64, Vec<NodeInfo>) {
        let request_id = self.next_request_id;
        self.next_request_id += 1;

        let seeds = self.routing.closest(&query.target_key(), K);
        let mut lookup = Lookup::new(query, seeds);
        let wave = lookup.next_wave();
        self.lookups.insert(request_id, lookup);
        (request_id, wave)
    }

    /// Feed a response into a lookup and get the next wave (empty when done).
    pub fn on_response(&mut self, response: &ResolveResponse) -> Vec<NodeInfo> {
        // Learn the keyspace peers the response mentioned, whatever it answered.
        // Both spaces at once: a referral that only reached the routing table
        // would leave the peer un-dialable and the walk would stall on it.
        for peer in &response.closer {
            self.peers.insert(peer.info.id, peer.agent_id);
            self.routing.insert(peer.info.clone());
        }
        let Some(lookup) = self.lookups.get_mut(&response.request_id) else {
            return Vec::new();
        };
        lookup.on_response(response);
        lookup.next_wave()
    }

    /// The outcome of a lookup, once it has one.
    pub fn lookup_outcome(&self, request_id: u64) -> Option<LookupOutcome> {
        self.lookups.get(&request_id)?.outcome()
    }

    /// The query a lookup is resolving — needed to build the request envelopes
    /// for each wave after the first.
    pub fn lookup_query(&self, request_id: u64) -> Option<ResolveQuery> {
        Some(self.lookups.get(&request_id)?.query().clone())
    }

    /// Ask a lookup for its next wave directly, without a response to absorb.
    /// Used when a wave could not be delivered and the walk must continue.
    pub fn next_wave(&mut self, request_id: u64) -> Vec<NodeInfo> {
        self.lookups
            .get_mut(&request_id)
            .map(|l| l.next_wave())
            .unwrap_or_default()
    }

    /// Record that a peer could not be reached, so the lookup stops waiting on
    /// it. Also drops it from the routing table: a node we cannot address is not
    /// a useful contact to offer anyone else.
    pub fn mark_unreachable(&mut self, request_id: u64, node: &NameKey) {
        if let Some(lookup) = self.lookups.get_mut(&request_id) {
            lookup.on_timeout(node);
        }
        self.routing.remove(node);
        self.peers.remove(node);
    }

    /// Drop a lookup's state without taking an outcome.
    pub fn abandon_lookup(&mut self, request_id: u64) -> bool {
        self.lookups.remove(&request_id).is_some()
    }

    /// Drop a finished lookup and take its outcome.
    pub fn finish_lookup(&mut self, request_id: u64) -> Option<LookupOutcome> {
        let outcome = self.lookups.get(&request_id)?.outcome()?;
        self.lookups.remove(&request_id);
        Some(outcome)
    }

    /// Lookups still in flight.
    pub fn active_lookups(&self) -> usize {
        self.lookups.len()
    }

    /// Drop expired records.
    pub fn sweep(&mut self, now: u64) -> usize {
        self.store.sweep_expired(now)
    }

    /// Whether this node is among the K closest to a key, i.e. whether it is
    /// one of the nodes responsible for storing that record.
    pub fn is_responsible_for(&self, key: &NameKey) -> bool {
        let closer = self
            .routing
            .closest(key, K)
            .into_iter()
            .filter(|n| n.id.distance(key) < self.local_key.distance(key))
            .count();
        closer < K
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use spine_name::Endpoint;

    fn record(seed: u8, seq: u64, now: u64, caps: &[&str]) -> NameRecord {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name, seq, now)
            .unwrap()
            .with_ttl(3600)
            .with_endpoint(Endpoint::new("tcp", format!("10.0.0.{seed}:9440")));
        for c in caps {
            rec = rec.with_capability(*c);
        }
        rec.sign(&key).unwrap();
        rec
    }

    fn node(seed: u8) -> NodeInfo {
        NodeInfo::new(NameKey::of(&[seed]), vec![format!("10.0.0.{seed}:9440")], 100)
    }

    /// A stable mesh identity per seed, so a test peer is addressable.
    fn agent(seed: u8) -> AgentId {
        AgentId(uuid::Uuid::from_u128(seed as u128))
    }

    /// The same node, paired with the mesh identity a referral has to carry.
    fn peer(seed: u8) -> KeyspacePeer {
        KeyspacePeer::new(node(seed), agent(seed))
    }

    #[test]
    fn capability_queries_route_to_a_stable_point_in_the_keyspace() {
        let a = ResolveQuery::Capability("web.search".into());
        let b = ResolveQuery::Capability("WEB.SEARCH".into());
        assert_eq!(a.target_key(), b.target_key(), "case must not split the key");
        assert_ne!(
            a.target_key(),
            ResolveQuery::Capability("web.crawl".into()).target_key()
        );
    }

    #[test]
    fn a_node_answers_from_its_own_store() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        let rec = record(1, 1, 1_000, &["web.search"]);
        svc.publish(rec.clone()).unwrap();

        let req = ResolveRequest {
            request_id: 7,
            query: ResolveQuery::Name(rec.name.clone()),
        };
        let resp = svc.handle_request(&req, 1_000);
        assert_eq!(resp.request_id, 7);
        assert_eq!(resp.record, Some(rec));
        assert!(resp.is_answer());
    }

    #[test]
    fn a_node_answers_capability_queries_without_a_central_index() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        svc.publish(record(1, 1, 1_000, &["web.search"])).unwrap();
        svc.publish(record(2, 1, 1_000, &["web.search"])).unwrap();
        svc.publish(record(3, 1, 1_000, &["data.analyze"])).unwrap();

        let resp = svc.handle_request(
            &ResolveRequest {
                request_id: 1,
                query: ResolveQuery::Capability("web.search".into()),
            },
            1_000,
        );
        assert_eq!(resp.providers.len(), 2);
        assert!(resp.is_answer());
    }

    #[test]
    fn a_node_that_lacks_the_answer_returns_only_strictly_closer_peers() {
        let local = NameKey::from_bytes([0xFFu8; 32]);
        let mut svc = NameService::new(local);
        for seed in 1..=30u8 {
            svc.add_peer(node(seed), agent(seed));
        }
        let wanted = SpineUri::did([1u8; 32]);
        let resp = svc.handle_request(
            &ResolveRequest {
                request_id: 1,
                query: ResolveQuery::Name(wanted.clone()),
            },
            1_000,
        );

        assert!(resp.record.is_none());
        assert!(!resp.closer.is_empty(), "a referral must be offered");
        let target = wanted.key();
        for n in &resp.closer {
            assert!(
                n.info.id.distance(&target) < local.distance(&target),
                "returning a non-closer node would stall the walk"
            );
        }
    }

    #[test]
    fn expired_records_are_not_served() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut rec = record(1, 1, 1_000, &[]).with_ttl(60);
        rec.sign(&key).unwrap();
        let name = rec.name.clone();
        svc.publish(rec).unwrap();

        let req = ResolveRequest {
            request_id: 1,
            query: ResolveQuery::Name(name),
        };
        assert!(svc.handle_request(&req, 1_030).record.is_some());
        assert!(svc.handle_request(&req, 1_100).record.is_none());
    }

    #[test]
    fn a_lookup_with_no_peers_terminates_immediately_as_not_found() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        let (id, wave) = svc.start_lookup(ResolveQuery::Name(SpineUri::did([1u8; 32])));
        assert!(wave.is_empty());
        assert_eq!(svc.lookup_outcome(id), Some(LookupOutcome::NotFound));
    }

    #[test]
    fn a_lookup_queries_the_closest_peers_first_and_at_most_alpha() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        for seed in 1..=30u8 {
            svc.add_node(node(seed));
        }
        let target = SpineUri::did([1u8; 32]);
        let (_, wave) = svc.start_lookup(ResolveQuery::Name(target.clone()));
        assert_eq!(wave.len(), ALPHA);

        let key = target.key();
        for pair in wave.windows(2) {
            assert!(pair[0].id.distance(&key) <= pair[1].id.distance(&key));
        }
    }

    #[test]
    fn a_valid_record_completes_the_lookup() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        for seed in 1..=10u8 {
            svc.add_node(node(seed));
        }
        let rec = record(1, 1, 1_000, &[]);
        let (id, wave) = svc.start_lookup(ResolveQuery::Name(rec.name.clone()));
        assert!(!wave.is_empty());

        let next = svc.on_response(&ResolveResponse {
            request_id: id,
            record: Some(rec.clone()),
            providers: vec![],
            closer: vec![],
        });
        assert!(next.is_empty(), "an answered lookup issues no more waves");
        assert_eq!(
            svc.finish_lookup(id),
            Some(LookupOutcome::Found(Box::new(rec)))
        );
        assert_eq!(svc.active_lookups(), 0);
    }

    #[test]
    fn a_forged_record_cannot_satisfy_a_lookup() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        svc.add_node(node(1));
        let mut forged = record(1, 1, 1_000, &[]);
        forged.capabilities.push("admin".into()); // breaks the signature
        let name = forged.name.clone();

        let (id, _) = svc.start_lookup(ResolveQuery::Name(name));
        svc.on_response(&ResolveResponse {
            request_id: id,
            record: Some(forged),
            providers: vec![],
            closer: vec![],
        });
        assert_ne!(
            svc.lookup_outcome(id),
            Some(LookupOutcome::Found(Box::new(record(1, 1, 1_000, &[]))))
        );
        // It either keeps walking or reports NotFound — never accepts the forgery.
        if let Some(outcome) = svc.lookup_outcome(id) {
            assert_eq!(outcome, LookupOutcome::NotFound);
        }
    }

    #[test]
    fn a_peer_cannot_redirect_a_lookup_with_a_valid_record_for_a_different_name() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        svc.add_node(node(1));
        let wanted = record(1, 1, 1_000, &[]);
        let other = record(2, 1, 1_000, &[]); // correctly signed, wrong name

        let (id, _) = svc.start_lookup(ResolveQuery::Name(wanted.name.clone()));
        svc.on_response(&ResolveResponse {
            request_id: id,
            record: Some(other),
            providers: vec![],
            closer: vec![],
        });
        if let Some(outcome) = svc.lookup_outcome(id) {
            assert_eq!(outcome, LookupOutcome::NotFound, "wrong-name record ignored");
        }
    }

    #[test]
    fn a_later_version_of_a_record_wins_during_a_lookup() {
        let mut lookup = Lookup::new(
            ResolveQuery::Name(record(1, 1, 1_000, &[]).name.clone()),
            vec![node(1)],
        );
        lookup.next_wave();
        lookup.on_response(&ResolveResponse {
            request_id: 1,
            record: Some(record(1, 1, 1_000, &[])),
            providers: vec![],
            closer: vec![],
        });
        lookup.on_response(&ResolveResponse {
            request_id: 1,
            record: Some(record(1, 5, 1_000, &[])),
            providers: vec![],
            closer: vec![],
        });
        match lookup.outcome().unwrap() {
            LookupOutcome::Found(r) => assert_eq!(r.seq, 5),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn capability_lookups_accumulate_providers_and_deduplicate() {
        let mut lookup = Lookup::new(
            ResolveQuery::Capability("web.search".into()),
            vec![node(1), node(2)],
        );
        lookup.next_wave();
        let a = record(1, 1, 1_000, &["web.search"]);
        let b = record(2, 1, 1_000, &["web.search"]);

        lookup.on_response(&ResolveResponse {
            request_id: 1,
            record: None,
            providers: vec![a.clone(), b.clone()],
            closer: vec![],
        });
        // The same providers arriving from a second peer must not double-count.
        lookup.on_response(&ResolveResponse {
            request_id: 1,
            record: None,
            providers: vec![a, b],
            closer: vec![],
        });
        assert_eq!(lookup.providers.len(), 2);
    }

    #[test]
    fn a_provider_lacking_the_capability_is_not_collected() {
        let mut lookup = Lookup::new(ResolveQuery::Capability("web.search".into()), vec![node(1)]);
        lookup.next_wave();
        lookup.on_response(&ResolveResponse {
            request_id: 1,
            record: None,
            providers: vec![record(1, 1, 1_000, &["data.analyze"])],
            closer: vec![],
        });
        assert!(lookup.providers.is_empty());
    }

    #[test]
    fn a_node_is_never_queried_twice() {
        let mut lookup = Lookup::new(ResolveQuery::Name(SpineUri::did([1u8; 32])), vec![node(1)]);
        let first = lookup.next_wave();
        assert_eq!(first.len(), 1);

        // The peer answers by re-advertising itself — a classic way to loop.
        lookup.on_response(&ResolveResponse {
            request_id: 1,
            record: None,
            providers: vec![],
            closer: vec![peer(1)],
        });
        assert!(
            lookup.next_wave().is_empty(),
            "re-offering a queried node must not produce a new wave"
        );
        assert_eq!(lookup.queried_count(), 1);
    }

    #[test]
    fn a_lookup_terminates_even_when_peers_never_get_closer() {
        // Adversarial: every peer answers with fresh, useless nodes, forever.
        let mut lookup = Lookup::new(ResolveQuery::Name(SpineUri::did([1u8; 32])), vec![node(1)]);
        let mut waves = 0;
        let mut seed = 100u8;

        while !lookup.is_done() {
            let wave = lookup.next_wave();
            if wave.is_empty() && lookup.in_flight() == 0 {
                break;
            }
            if !wave.is_empty() {
                waves += 1;
                assert!(waves <= MAX_ROUNDS, "lookup failed to terminate");
            }
            // Every node that was asked answers — so `in_flight` always drains
            // and the walk can only be ended by running out of candidates or by
            // the round cap, never by a lost response.
            for _ in 0..wave.len().max(1) {
                if lookup.in_flight() == 0 {
                    break;
                }
                let fresh: Vec<KeyspacePeer> = (0..3)
                    .map(|_| {
                        seed = seed.wrapping_add(1);
                        peer(seed)
                    })
                    .collect();
                lookup.on_response(&ResolveResponse {
                    request_id: 1,
                    record: None,
                    providers: vec![],
                    closer: fresh,
                });
            }
        }

        assert!(lookup.is_done(), "an adversarial peer must not stall a lookup");
        assert_eq!(lookup.outcome(), Some(LookupOutcome::NotFound));
        assert_eq!(lookup.in_flight(), 0, "no request left dangling");
    }

    #[test]
    fn a_lookup_waits_for_peers_still_in_flight_before_declaring_itself_done() {
        // Regression: a capability query used to settle the instant its
        // shortlist ran dry, discarding answers from peers still outstanding —
        // silently truncating the result to whatever the fastest peer knew.
        let mut lookup = Lookup::new(
            ResolveQuery::Capability("web.search".into()),
            vec![node(1), node(2)],
        );
        let wave = lookup.next_wave();
        assert_eq!(wave.len(), 2);
        assert_eq!(lookup.in_flight(), 2);

        // First peer answers. The shortlist is now empty, but one peer is still
        // outstanding, so the lookup must NOT be finished.
        lookup.on_response(&ResolveResponse {
            request_id: 1,
            record: None,
            providers: vec![record(1, 1, 1_000, &["web.search"])],
            closer: vec![],
        });
        assert!(lookup.next_wave().is_empty());
        assert_eq!(lookup.in_flight(), 1);
        assert!(!lookup.is_done(), "must not settle while a peer is outstanding");

        // Second peer answers; now it is genuinely done, with both providers.
        lookup.on_response(&ResolveResponse {
            request_id: 1,
            record: None,
            providers: vec![record(2, 1, 1_000, &["web.search"])],
            closer: vec![],
        });
        assert!(lookup.next_wave().is_empty());
        assert!(lookup.is_done());
        match lookup.outcome().unwrap() {
            LookupOutcome::Providers(p) => assert_eq!(p.len(), 2, "no answer was dropped"),
            other => panic!("expected Providers, got {other:?}"),
        }
    }

    #[test]
    fn an_unreachable_peer_releases_its_in_flight_slot() {
        let mut lookup = Lookup::new(ResolveQuery::Name(SpineUri::did([1u8; 32])), vec![node(1)]);
        lookup.next_wave();
        assert_eq!(lookup.in_flight(), 1);

        lookup.on_timeout(&node(1).id);
        assert_eq!(lookup.in_flight(), 0);
        // With nothing outstanding and nothing left to try, it settles.
        assert!(lookup.next_wave().is_empty());
        assert!(lookup.is_done());
        assert_eq!(lookup.outcome(), Some(LookupOutcome::NotFound));
    }

    #[test]
    fn a_locally_held_record_answers_without_starting_a_lookup() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        let rec = record(1, 1, 1_000, &["web.search"]);
        svc.publish(rec.clone()).unwrap();

        assert_eq!(
            svc.resolve_locally(&ResolveQuery::Name(rec.name.clone()), 1_000),
            Some(LookupOutcome::Found(Box::new(rec)))
        );
        // An empty local index is not an answer — the mesh may know more.
        assert_eq!(
            svc.resolve_locally(&ResolveQuery::Capability("nothing".into()), 1_000),
            None
        );
        // Nor is a stale one.
        assert_eq!(
            svc.resolve_locally(&ResolveQuery::Capability("web.search".into()), 999_999),
            None
        );
    }

    #[test]
    fn a_timeout_removes_a_peer_from_consideration() {
        let mut lookup = Lookup::new(ResolveQuery::Name(SpineUri::did([1u8; 32])), vec![node(1), node(2)]);
        lookup.next_wave();
        lookup.on_timeout(&node(1).id);
        assert!(!lookup.shortlist.iter().any(|n| n.id == node(1).id));
    }

    #[test]
    fn responses_teach_the_routing_table_even_when_they_do_not_answer() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        svc.add_node(node(1));
        let (id, _) = svc.start_lookup(ResolveQuery::Name(SpineUri::did([1u8; 32])));
        assert_eq!(svc.node_count(), 1);

        svc.on_response(&ResolveResponse {
            request_id: id,
            record: None,
            providers: vec![],
            closer: vec![peer(2), peer(3)],
        });
        assert_eq!(svc.node_count(), 3, "the mesh learns from every exchange");
        assert_eq!(
            svc.addressable_peers(),
            2,
            "a referral must arrive dialable, not merely positioned"
        );
    }

    #[test]
    fn a_response_to_an_unknown_request_is_ignored_safely() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        let next = svc.on_response(&ResolveResponse::empty(999));
        assert!(next.is_empty());
    }

    #[test]
    fn responsibility_narrows_as_the_neighbourhood_fills() {
        let mut svc = NameService::new(NameKey::from_bytes([0u8; 32]));
        let key = NameKey::from_bytes([0xFFu8; 32]);
        // With no peers, this node is trivially responsible.
        assert!(svc.is_responsible_for(&key));

        // Surround the key with K closer nodes; responsibility passes to them.
        for i in 0..(K as u16 + 5) {
            let mut bytes = [0xFFu8; 32];
            bytes[31] = i as u8;
            svc.add_node(NodeInfo::new(NameKey::from_bytes(bytes), vec![], 100));
        }
        assert!(!svc.is_responsible_for(&key));
    }

    #[test]
    fn a_mesh_node_places_itself_in_the_keyspace_by_its_signing_key() {
        use crate::identity::SigningIdentity;
        use crate::mesh::{MeshConfig, MeshNode};

        let identity = SigningIdentity::from_seed("resolver", [3u8; 32]);
        let expected = *identity.public_key();
        let node = MeshNode::new(identity, MeshConfig::default());

        assert_eq!(node.name_key(), NameKey::from_bytes(expected));
        let info = node.as_name_node(vec!["10.0.0.9:9440".into()], 500);
        assert_eq!(info.id, node.name_key());
        assert_eq!(info.last_seen, 500);
    }

    #[test]
    fn name_traffic_travels_as_signed_mesh_envelopes() {
        use crate::identity::SigningIdentity;
        use crate::mesh::{MeshConfig, MeshNode, MeshPayload, MeshTarget};

        let node = MeshNode::new(
            SigningIdentity::from_seed("publisher", [4u8; 32]),
            MeshConfig::default(),
        );
        let rec = record(1, 1, 1_000, &["web.search"]);

        let envelope = node.announce_name(rec.clone()).unwrap();
        assert_eq!(envelope.to, MeshTarget::Broadcast);
        match &envelope.payload {
            MeshPayload::NameAnnounce(a) => assert_eq!(a.record, rec),
            other => panic!("expected NameAnnounce, got {other:?}"),
        }

        // A request and its answer are ordinary addressed envelopes.
        let peer = *node.agent_id();
        let req = node.name_resolve_request(peer, 42, ResolveQuery::Capability("web.search".into()));
        assert_eq!(req.to, MeshTarget::Agent(peer));
        match &req.payload {
            MeshPayload::NameResolveRequest(r) => assert_eq!(r.request_id, 42),
            other => panic!("expected NameResolveRequest, got {other:?}"),
        }

        let resp = node.name_resolve_response(peer, ResolveResponse::empty(42));
        assert!(matches!(resp.payload, MeshPayload::NameResolveResponse(_)));
    }

    #[test]
    fn an_unverifiable_record_cannot_be_announced_to_the_mesh() {
        use crate::identity::SigningIdentity;
        use crate::mesh::{MeshConfig, MeshNode};

        let node = MeshNode::new(
            SigningIdentity::from_seed("publisher", [5u8; 32]),
            MeshConfig::default(),
        );
        let mut forged = record(1, 1, 1_000, &[]);
        forged.capabilities.push("admin".into());

        assert!(
            node.announce_name(forged).is_err(),
            "the mesh must not carry a record that does not verify"
        );
    }

    /// The end-to-end proof: three nodes, none of which knows where a record
    /// lives, converge on it by walking the keyspace.
    #[test]
    fn a_lookup_converges_across_a_simulated_mesh() {
        let holder_key = NameKey::of(b"holder");
        let middle_key = NameKey::of(b"middle");
        let seeker_key = NameKey::of(b"seeker");

        let rec = record(1, 1, 1_000, &["web.search"]);

        // The holder stores the record.
        let mut holder = NameService::new(holder_key);
        holder.publish(rec.clone()).unwrap();

        // The middle node knows the holder but not the record.
        let mut middle = NameService::new(middle_key);
        middle.add_node(NodeInfo::new(holder_key, vec!["holder:1".into()], 100));

        // The seeker knows only the middle node.
        let mut seeker = NameService::new(seeker_key);
        seeker.add_node(NodeInfo::new(middle_key, vec!["middle:1".into()], 100));

        let (id, wave) = seeker.start_lookup(ResolveQuery::Name(rec.name.clone()));
        assert_eq!(wave.len(), 1);
        assert_eq!(wave[0].id, middle_key);

        // Hop 1: ask the middle node, which points at the holder.
        let mut resp = middle.handle_request(
            &ResolveRequest {
                request_id: id,
                query: ResolveQuery::Name(rec.name.clone()),
            },
            1_000,
        );
        assert!(resp.record.is_none());
        // Force the referral even if the holder is not numerically closer: the
        // point under test is that the seeker follows referrals it is given.
        resp.closer = vec![KeyspacePeer::new(
            NodeInfo::new(holder_key, vec!["holder:1".into()], 100),
            agent(9),
        )];

        let wave = seeker.on_response(&resp);
        assert_eq!(wave.len(), 1, "the seeker follows the referral");
        assert_eq!(wave[0].id, holder_key);

        // Hop 2: the holder answers.
        let resp = holder.handle_request(
            &ResolveRequest {
                request_id: id,
                query: ResolveQuery::Name(rec.name.clone()),
            },
            1_000,
        );
        assert_eq!(resp.record.as_ref(), Some(&rec));
        seeker.on_response(&resp);

        assert_eq!(
            seeker.finish_lookup(id),
            Some(LookupOutcome::Found(Box::new(rec))),
            "the seeker resolved a name it had never seen, from a node it had never met"
        );
    }

    /// A node query has no answer to stop on, so a peer volunteering a record
    /// must not be able to end the walk early. Storing a record depends on this:
    /// a walk cut short reports a neighbourhood it never reached.
    #[test]
    fn a_node_query_is_not_satisfied_by_a_record() {
        let target = NameKey::of(b"somewhere");
        let mut lookup = Lookup::new(ResolveQuery::Node(target), vec![node(1), node(2)]);
        let wave = lookup.next_wave();
        assert!(!wave.is_empty());

        let mut response = ResolveResponse::empty(1);
        response.record = Some(record(9, 1, 100, &[]));
        lookup.on_response(&response);

        assert!(!lookup.is_done(), "a record is not an answer to a node query");
        assert!(lookup.outcome().is_none());
    }

    /// The result of a node query is the converged shortlist, nearest-first.
    #[test]
    fn a_node_query_returns_the_closest_peers_it_found() {
        let target = NameKey::of(b"somewhere");
        let mut lookup = Lookup::new(ResolveQuery::Node(target), vec![node(1), node(2)]);

        // Query and time out both seeds, so the walk runs itself out.
        while !lookup.next_wave().is_empty() {}
        lookup.on_timeout(&node(1).id);
        lookup.on_timeout(&node(2).id);
        while !lookup.next_wave().is_empty() {}

        match lookup.outcome() {
            Some(LookupOutcome::Closest(_)) => {}
            other => panic!("expected a closest-peers outcome, got {other:?}"),
        }
    }

    /// Replicas go to peers that can be reached. A peer with no mesh identity
    /// cannot be sent to, and one of them is this node itself.
    #[test]
    fn closest_peers_skips_this_node_and_anything_unaddressable() {
        let mut service = NameService::new(NameKey::of(&[1]));
        service.add_peer(node(2), agent(2));
        // Known in the keyspace, but nothing can address it.
        service.add_node(node(3));

        let peers = service.closest_peers(&NameKey::of(b"target"), K);
        assert_eq!(peers.len(), 1, "only the addressable peer: {peers:?}");
        assert_eq!(peers[0].key(), node(2).id);
    }

    /// Re-offering a record cannot extend its life, so a record near expiry is
    /// something to report upward rather than something to fix here.
    #[test]
    fn a_record_close_to_expiry_is_reported_as_lapsing() {
        let mut service = NameService::new(NameKey::of(&[1]));
        let rec = record(4, 1, 1_000, &[]); // ttl 3600
        let name = rec.name.clone();
        service.publish(rec).unwrap();

        assert!(service.lapsing(1_000, 900).is_empty(), "not yet");
        assert_eq!(service.lapsing(4_000, 900), vec![name], "600s left");
        assert!(
            service.lapsing(9_000, 900).is_empty(),
            "already expired, so past reporting"
        );
    }

    /// A record already held at an equal or newer version must not be treated as
    /// news, or two nodes holding different versions announce at each other
    /// without end.
    #[test]
    fn re_offering_the_same_record_is_reported_as_superseded() {
        let mut service = NameService::new(NameKey::of(&[1]));
        let rec = record(4, 1, 1_000, &[]);
        assert_eq!(
            service.publish(rec.clone()).unwrap(),
            spine_name::PutOutcome::Inserted
        );
        assert_eq!(
            service.publish(rec).unwrap(),
            spine_name::PutOutcome::Superseded
        );
    }
}
