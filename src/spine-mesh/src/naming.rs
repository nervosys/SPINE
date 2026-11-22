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

/// Most keyspace-to-mesh identity mappings a [`NameService`] remembers. Like the
/// transport's contact registries, the budget is fixed so that peers learned
/// from the network cannot grow it without limit. When it is full, learning a
/// new peer evicts the one whose mapping was least recently established or
/// refreshed by this node (and drops it from the routing table too).
pub const MAX_PEERS: usize = 1024;

/// Distinct peers that must have answered a name lookup before it may settle on
/// the best record seen. One peer holding an old, validly signed record must not
/// be able to win the race and roll a name back, so the first answer is never
/// final. The effective target is `min(QUORUM, peers that can still answer)`, so
/// a small mesh settles as soon as everyone reachable has been heard.
pub const QUORUM: usize = 3;

/// Most endpoints kept per referred or announced node; the rest are dropped.
pub const MAX_NODE_ENDPOINTS: usize = 8;

/// Longest endpoint string kept, in bytes; a longer one is dropped.
pub const MAX_ENDPOINT_BYTES: usize = 256;

/// Bound an endpoint list received from a peer: over-long entries are dropped
/// and at most [`MAX_NODE_ENDPOINTS`] survive. Excess is discarded rather than
/// failing the message that carried it.
pub fn bound_endpoints(endpoints: &[String]) -> Vec<String> {
    endpoints
        .iter()
        .filter(|e| e.len() <= MAX_ENDPOINT_BYTES)
        .take(MAX_NODE_ENDPOINTS)
        .cloned()
        .collect()
}

/// Bound a referral list received from a peer: at most [`K`] nodes, each with
/// its endpoints bounded by [`bound_endpoints`].
pub fn bound_referrals(closer: &[KeyspacePeer]) -> Vec<KeyspacePeer> {
    closer
        .iter()
        .take(K)
        .map(|p| {
            KeyspacePeer::new(
                NodeInfo::new(
                    p.info.id,
                    bound_endpoints(&p.info.endpoints),
                    p.info.last_seen,
                ),
                p.agent_id,
            )
        })
        .collect()
}

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
    /// are, and a name lookup cannot tell you. A name lookup settles once a
    /// quorum has answered with a record — usually long before the walk has
    /// converged on the neighbourhood the record belongs in.
    Node(NameKey),
}

/// Most capability keys a record is placed at, besides its own name key.
///
/// A record may advertise arbitrarily many terms, and each key costs a
/// keyspace walk plus up to [`K`] sends on the publisher and a stored copy on
/// the receivers. The first `MAX_CAPABILITY_KEYS` terms in sorted order are
/// placed, so the choice is deterministic and a receiver computes the same set
/// the publisher did. Terms beyond the cap are discoverable only through the
/// nodes that happen to hold the record.
pub const MAX_CAPABILITY_KEYS: usize = 8;

/// The keyspace point a capability term routes to (case-insensitive).
pub fn capability_key(term: &str) -> NameKey {
    NameKey::of(format!("cap:{}", term.to_ascii_lowercase()).as_bytes())
}

/// The capability keys a record is placed at: one per advertised term, sorted
/// and deduplicated, capped at [`MAX_CAPABILITY_KEYS`].
pub fn capability_placement_keys(record: &NameRecord) -> Vec<NameKey> {
    let mut terms: Vec<String> = record
        .capabilities
        .iter()
        .map(|t| t.to_ascii_lowercase())
        .collect();
    terms.sort();
    terms.dedup();
    terms
        .into_iter()
        .take(MAX_CAPABILITY_KEYS)
        .map(|t| capability_key(&t))
        .collect()
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
            ResolveQuery::Capability(term) => capability_key(term),
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
    /// Peers that actually answered. Zero at the end of a walk means nothing was
    /// learned, which is not the same as the keyspace confirming absence.
    answered: usize,
    /// Set once no unqueried candidate remains, nothing is in flight, or the
    /// round cap is hit.
    exhausted: bool,
    /// The caller's clock, for rejecting records already expired at lookup time.
    /// Zero (the default) means no clock was supplied, so nothing is stale.
    now: u64,
    /// Queried peers that have neither answered nor timed out. Lets an
    /// identity-checked response count once, and only if it was asked.
    awaiting: HashSet<NameKey>,
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
            answered: 0,
            exhausted: false,
            now: 0,
            awaiting: HashSet::new(),
        };
        lookup.absorb_nodes(seeds);
        lookup
    }

    /// Supply the caller's clock. A record expired at `now` is never accepted as
    /// an answer (it still counts as its peer having answered); `blob:` names
    /// are immutable and never expire.
    pub fn set_clock(&mut self, now: u64) {
        self.now = now;
    }

    /// Whether a record is stale at the lookup's clock.
    fn is_stale(&self, record: &NameRecord) -> bool {
        !record.name.is_immutable() && record.is_expired(self.now)
    }

    /// Size, lifetime and (when a clock was supplied) post-dating checks on a
    /// record a peer returned. Without a clock the bound on `published_at`
    /// cannot be applied, the same way nothing counts as stale.
    fn admissible(&self, record: &NameRecord) -> bool {
        record.validate_size().is_ok()
            && record.validate_lifetime().is_ok()
            && (self.now == 0 || record.validate_not_postdated(self.now).is_ok())
    }

    /// Candidates not yet queried.
    fn unqueried(&self) -> usize {
        self.shortlist
            .iter()
            .filter(|n| !self.queried.contains(&n.id))
            .count()
    }

    /// A name lookup's settling rule: some record has been accepted, and at
    /// least `min(QUORUM, answered + in flight + unqueried)` peers have answered.
    /// Peers still in flight or not yet asked count as able to answer, so the
    /// walk keeps going while the quorum is attainable and stops as soon as it
    /// is met or no one else can be heard.
    fn name_settled(&self) -> bool {
        if self.record.is_none() {
            return false;
        }
        let reachable = self.answered + self.in_flight + self.unqueried();
        self.answered >= QUORUM.min(reachable)
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
            self.awaiting.insert(node.id);
        }
        self.in_flight += wave.len();
        self.rounds += 1;
        wave
    }

    /// Absorb a peer's response.
    ///
    /// Counts one answer without knowing who sent it, so it trusts the caller to
    /// deliver each peer's response once. Anything reading responses off the
    /// wire should use [`Lookup::on_response_from`].
    pub fn on_response(&mut self, response: &ResolveResponse) {
        self.in_flight = self.in_flight.saturating_sub(1);
        self.answered += 1;
        self.absorb(response);
    }

    /// Absorb a response from a known sender. `from` are the keyspace positions
    /// the sender may occupy; the response counts only if one of them was asked
    /// and has not yet answered, and then counts exactly once. Returns whether
    /// it was accepted. This is what makes "distinct peers answered" true: a
    /// peer cannot meet the quorum by answering repeatedly, and an unsolicited
    /// response cannot count at all.
    pub fn on_response_from(&mut self, from: &[NameKey], response: &ResolveResponse) -> bool {
        let Some(key) = from.iter().find(|k| self.awaiting.contains(*k)).copied() else {
            return false;
        };
        self.awaiting.remove(&key);
        self.in_flight = self.in_flight.saturating_sub(1);
        self.answered += 1;
        self.absorb(response);
        true
    }

    fn absorb(&mut self, response: &ResolveResponse) {
        // A record only counts if it verifies, actually answers *this* query and
        // is not already expired. A peer that returns someone else's record, a
        // forgery or a lapsed one must not be able to redirect the lookup; the
        // peer still counts as having answered.
        if let Some(record) = &response.record {
            if self.admissible(record)
                && !self.is_stale(record)
                && record.verify().is_ok()
                && self.matches_query(record)
            {
                let better = match &self.record {
                    Some(existing) => record.supersedes(existing),
                    None => true,
                };
                if better {
                    self.record = Some(record.clone());
                }
            }
        }

        // A response never needs more than K providers, so a longer list is
        // padding, and each entry costs a signature check.
        for provider in response.providers.iter().take(K) {
            if self.admissible(provider)
                && !self.is_stale(provider)
                && provider.verify().is_ok()
                && self.wants_capability(provider)
                && !self.providers.iter().any(|p| p.name == provider.name)
            {
                self.providers.push(provider.clone());
            }
        }

        self.absorb_nodes(
            bound_referrals(&response.closer)
                .into_iter()
                .map(|p| p.info)
                .collect(),
        );
    }

    /// Mark a node as unreachable so the lookup stops waiting on it.
    pub fn on_timeout(&mut self, node: &NameKey) {
        if self.awaiting.remove(node) {
            self.in_flight = self.in_flight.saturating_sub(1);
        }
        self.queried.insert(*node);
        self.shortlist.retain(|n| &n.id != node);
    }

    /// Peers that have answered this lookup, with or without the record.
    pub fn answered(&self) -> usize {
        self.answered
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
            // A name resolves to exactly one record, but the first answer is
            // not trusted to be the newest: settle on the best record seen once
            // the quorum has answered (see `name_settled`).
            ResolveQuery::Name(_) => self.name_settled(),
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

/// A remembered mesh identity, stamped with this node's own logical clock.
#[derive(Debug, Clone, Copy)]
struct PeerSlot {
    agent: AgentId,
    /// Value of `NameService::peer_clock` when last established or refreshed.
    /// Local and strictly increasing, so eviction order is deterministic and
    /// cannot be influenced by anything a peer says.
    touched: u64,
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
    ///
    /// Bounded by [`MAX_PEERS`].
    peers: HashMap<NameKey, PeerSlot>,
    /// Logical clock ordering `peers` updates, for eviction.
    peer_clock: u64,
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
            store: spine_name::RecordStore::with_limits(local_key, spine_name::DEFAULT_CAPACITY),
            routing: RoutingTable::new(local_key),
            peers: HashMap::new(),
            peer_clock: 0,
            lookups: HashMap::new(),
            next_request_id: 1,
        }
    }

    /// This node's keyspace position.
    pub fn local_key(&self) -> &NameKey {
        &self.local_key
    }

    /// Store a signed record at time `now` (the caller's clock, Unix seconds).
    /// Verification happens inside the store, so an unverifiable announcement
    /// cannot enter.
    ///
    /// The outcome matters to whatever is above this: a record that was
    /// superseded by one already held must not be replicated onward, or two
    /// nodes holding different versions will announce at each other forever.
    /// That includes a record a held tombstone outranks.
    pub fn publish(
        &mut self,
        record: NameRecord,
        now: u64,
    ) -> Result<spine_name::PutOutcome, spine_name::NameError> {
        // The store verifies the signature; size, lifetime and the bound on
        // `published_at` are checked here so a record a peer would refuse is
        // never stored or replicated.
        record.validate(now)?;
        self.store.put(record, now)
    }

    /// Accept a record announced by someone else.
    ///
    /// Distinct from [`NameService::publish`], which is how a node stores what
    /// it is itself publishing, and which is unconditional — a node must be able
    /// to serve its own names whether or not it happens to sit near their keys.
    ///
    /// An announcement is different. A signature proves only that the publisher
    /// holds the key for that name, and minting a `did:` name costs one keygen,
    /// so "it verified" is no reason to spend memory on it. A node keeps what it
    /// is near: records whose keys land outside its neighbourhood are refused,
    /// because a lookup for such a key would never be routed here and the copy
    /// would be pure cost.
    pub fn accept_announcement(
        &mut self,
        record: NameRecord,
        now: u64,
    ) -> Result<spine_name::PutOutcome, spine_name::NameError> {
        // Cheapest checks first: refuse a record whose size, lifetime or
        // publication time alone disqualifies it. Tombstones travel this path
        // like any record: they have no endpoints and no capabilities, and
        // responsibility is judged by their name key.
        record.validate(now)?;
        // A record is also placed near each capability key it advertises, so a
        // capability walk converges on nodes that hold it. Responsibility for
        // any of those keys suffices; the set is capped, so a record cannot
        // claim a place in more than a few neighbourhoods.
        let responsible = self.is_responsible_for(&record.name.key())
            || capability_placement_keys(&record)
                .iter()
                .any(|k| self.is_responsible_for(k));
        if !responsible {
            return Ok(spine_name::PutOutcome::Rejected);
        }
        self.store.put(record, now)
    }

    /// Learn about a keyspace peer whose mesh identity is not known.
    ///
    /// Such a peer can be *referred to* but not dialed, so prefer
    /// [`NameService::add_peer`] wherever the agent id is available.
    pub fn add_node(&mut self, node: NodeInfo) -> bool {
        self.routing.insert(node)
    }

    /// Learn about a keyspace peer in both routing spaces at once.
    ///
    /// For a peer this node has itself heard from or authenticated, not one
    /// merely referred by someone else: `node.last_seen` must be the caller's
    /// local clock, and it refreshes any existing entry. The identity mapping is
    /// bounded by [`MAX_PEERS`]; see there for the eviction rule.
    pub fn add_peer(&mut self, node: NodeInfo, agent_id: AgentId) -> bool {
        let now = node.last_seen;
        self.remember_peer(node.id, agent_id);
        self.routing.insert_observed(node, now)
    }

    /// Record `key -> agent`, evicting the least recently touched mapping first
    /// if a new key would exceed [`MAX_PEERS`].
    fn remember_peer(&mut self, key: NameKey, agent: AgentId) {
        self.peer_clock += 1;
        let touched = self.peer_clock;
        if !self.peers.contains_key(&key) && self.peers.len() >= MAX_PEERS {
            let oldest = self
                .peers
                .iter()
                .min_by_key(|(_, slot)| slot.touched)
                .map(|(k, _)| *k);
            if let Some(oldest) = oldest {
                self.peers.remove(&oldest);
                self.routing.remove(&oldest);
            }
        }
        self.peers.insert(key, PeerSlot { agent, touched });
    }

    /// The mesh identity of a keyspace peer, if this node knows it.
    pub fn agent_for(&self, key: &NameKey) -> Option<AgentId> {
        self.peers.get(key).map(|slot| slot.agent)
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
                    .map(|slot| KeyspacePeer::new(n, slot.agent))
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

    /// Records this node is one of the nodes responsible for, in a stable order.
    ///
    /// Maintenance walks the keyspace once per record it re-offers, so the set
    /// it iterates has to be the set that is actually this node's to keep alive.
    /// A record held here while K nearer nodes exist is a copy that lookups will
    /// never route to; re-offering it spends a walk to tell the rightful holders
    /// something they already know, and every node doing that multiplies the
    /// traffic by however many stale copies the mesh is carrying.
    ///
    /// Ordered by key so a bounded pass can resume where the last one stopped
    /// rather than starving the tail of the store.
    pub fn records_to_maintain(&self, now: u64) -> Vec<NameRecord> {
        let mut records: Vec<NameRecord> = self
            .store
            .records()
            .filter(|r| !r.is_expired(now))
            .filter(|r| self.is_responsible_for(&r.name.key()))
            .cloned()
            .collect();
        records.sort_by(|a, b| a.name.key().as_bytes().cmp(b.name.key().as_bytes()));
        records
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
        self.start_lookup_at(query, 0)
    }

    /// Begin a lookup with the caller's freshness clock.
    pub fn start_lookup_at(&mut self, query: ResolveQuery, now: u64) -> (u64, Vec<NodeInfo>) {
        let request_id = self.next_request_id;
        self.next_request_id += 1;

        let seeds = self.routing.closest(&query.target_key(), K);
        let mut lookup = Lookup::new(query.clone(), seeds);
        lookup.set_clock(now);
        if let ResolveQuery::Capability(term) = &query {
            let mut local = ResolveResponse::empty(request_id);
            local.providers = self
                .store
                .providers_of(term, now)
                .into_iter()
                .cloned()
                .collect();
            lookup.on_response(&local);
        }
        let wave = lookup.next_wave();
        self.lookups.insert(request_id, lookup);
        (request_id, wave)
    }

    /// Feed a response into a lookup and get the next wave (empty when done).
    ///
    /// Does not check who sent the response; anything reading responses off the
    /// wire should use [`NameService::on_response_from`].
    pub fn on_response(&mut self, response: &ResolveResponse) -> Vec<NodeInfo> {
        self.learn_referrals(&response.closer);
        let Some(lookup) = self.lookups.get_mut(&response.request_id) else {
            return Vec::new();
        };
        lookup.on_response(response);
        lookup.next_wave()
    }

    /// Like [`NameService::on_response`], for a response whose sender is known.
    ///
    /// Returns `None` (and learns nothing) when the response is unsolicited: no
    /// such lookup, or the sender was not asked or has already answered. That is
    /// what keeps one peer from counting more than once toward a lookup's
    /// quorum. Otherwise returns the next wave, empty when the lookup is done.
    ///
    /// `from` is taken at its word: it is an unauthenticated agent id. Anything
    /// reading envelopes off the wire must verify the sender first and use
    /// [`NameService::on_response_from_keys`] (as the mesh resolver does).
    pub fn on_response_from(
        &mut self,
        from: AgentId,
        response: &ResolveResponse,
    ) -> Option<Vec<NodeInfo>> {
        let keys = self.keys_for_agent(from);
        self.on_response_from_keys(&keys, response)
    }

    /// Every keyspace position this node has recorded for a mesh identity.
    ///
    /// An agent id is only a label: nothing ties it to a key except whoever told
    /// us the pairing (a hello or ack, a registration, or an unsigned referral).
    /// A caller that needs to know the sender must verify the envelope signature
    /// against one of these keys, not trust the label.
    pub fn keys_for_agent(&self, agent: AgentId) -> Vec<NameKey> {
        self.peers
            .iter()
            .filter(|(_, slot)| slot.agent == agent)
            .map(|(key, _)| *key)
            .collect()
    }

    /// Like [`NameService::on_response_from`], for a response whose sender has
    /// been *proven* to hold each of `keys` (the envelope verified against them).
    /// Only those keys can satisfy a pending request.
    pub fn on_response_from_keys(
        &mut self,
        keys: &[NameKey],
        response: &ResolveResponse,
    ) -> Option<Vec<NodeInfo>> {
        let lookup = self.lookups.get_mut(&response.request_id)?;
        if !lookup.on_response_from(keys, response) {
            return None;
        }
        self.learn_referrals(&response.closer);
        Some(
            self.lookups
                .get_mut(&response.request_id)
                .map(|l| l.next_wave())
                .unwrap_or_default(),
        )
    }

    /// Learn the keyspace peers a response mentioned, whatever it answered.
    /// Both spaces at once: a referral that only reached the routing table
    /// would leave the peer un-dialable and the walk would stall on it.
    ///
    /// These are referrals, not first-hand contacts: they may fill free
    /// routing slots but never displace or redirect a contact already known,
    /// and only peers the routing table kept get an identity mapping. The list
    /// is bounded first (see [`bound_referrals`]).
    fn learn_referrals(&mut self, closer: &[KeyspacePeer]) {
        for peer in bound_referrals(closer) {
            let kept = self.routing.insert(peer.info.clone());
            if kept && !self.peers.contains_key(&peer.info.id) {
                self.remember_peer(peer.info.id, peer.agent_id);
            }
        }
    }

    /// Requests a lookup has sent that are neither answered nor timed out; zero
    /// for an unknown lookup.
    pub fn lookup_in_flight(&self, request_id: u64) -> usize {
        self.lookups
            .get(&request_id)
            .map_or(0, |lookup| lookup.in_flight())
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
        self.finish_lookup_counted(request_id).map(|(o, _)| o)
    }

    /// Like [`NameService::finish_lookup`], also reporting whether any peer
    /// answered. A `NotFound` with no answers means nobody could be asked.
    pub fn finish_lookup_counted(&mut self, request_id: u64) -> Option<(LookupOutcome, bool)> {
        let lookup = self.lookups.get(&request_id)?;
        let outcome = lookup.outcome()?;
        let answered = lookup.answered() > 0;
        self.lookups.remove(&request_id);
        Some((outcome, answered))
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
        NodeInfo::new(
            NameKey::of(&[seed]),
            vec![format!("10.0.0.{seed}:9440")],
            100,
        )
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
        assert_eq!(
            a.target_key(),
            b.target_key(),
            "case must not split the key"
        );
        assert_ne!(
            a.target_key(),
            ResolveQuery::Capability("web.crawl".into()).target_key()
        );
    }

    #[test]
    fn a_node_answers_from_its_own_store() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        let rec = record(1, 1, 1_000, &["web.search"]);
        svc.publish(rec.clone(), 1_000).unwrap();

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
        svc.publish(record(1, 1, 1_000, &["web.search"]), 1_000)
            .unwrap();
        svc.publish(record(2, 1, 1_000, &["web.search"]), 1_000)
            .unwrap();
        svc.publish(record(3, 1, 1_000, &["data.analyze"]), 1_000)
            .unwrap();

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
        svc.publish(rec, 1_000).unwrap();

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
    fn a_valid_record_completes_the_lookup_once_a_quorum_has_answered() {
        // Changed from "the first valid record ends it": a single answer can be
        // a stale record, so the lookup now waits for QUORUM peers.
        let mut svc = NameService::new(NameKey::of(b"local"));
        for seed in 1..=10u8 {
            svc.add_node(node(seed));
        }
        let rec = record(1, 1, 1_000, &[]);
        let (id, wave) = svc.start_lookup(ResolveQuery::Name(rec.name.clone()));
        assert!(!wave.is_empty());

        let answer = ResolveResponse {
            request_id: id,
            record: Some(rec.clone()),
            providers: vec![],
            closer: vec![],
        };
        for _ in 0..(QUORUM - 1) {
            svc.on_response(&answer);
            assert!(
                svc.lookup_outcome(id).is_none(),
                "fewer than QUORUM answers must not end the lookup"
            );
        }
        let next = svc.on_response(&answer);
        assert!(next.is_empty(), "a settled lookup issues no more waves");
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
            assert_eq!(
                outcome,
                LookupOutcome::NotFound,
                "wrong-name record ignored"
            );
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

        assert!(
            lookup.is_done(),
            "an adversarial peer must not stall a lookup"
        );
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
        assert!(
            !lookup.is_done(),
            "must not settle while a peer is outstanding"
        );

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
    fn a_lookup_reports_whether_any_peer_answered() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        svc.add_node(node(1));
        let (id, wave) = svc.start_lookup(ResolveQuery::Name(SpineUri::did([1u8; 32])));
        assert_eq!(wave.len(), 1);
        svc.mark_unreachable(id, &wave[0].id);
        assert!(svc.next_wave(id).is_empty());
        assert_eq!(
            svc.finish_lookup_counted(id),
            Some((LookupOutcome::NotFound, false)),
            "nobody answered: nothing was learned"
        );

        svc.add_node(node(2));
        let (id, wave) = svc.start_lookup(ResolveQuery::Name(SpineUri::did([1u8; 32])));
        assert_eq!(wave.len(), 1);
        svc.on_response(&ResolveResponse {
            request_id: id,
            record: None,
            providers: vec![],
            closer: vec![],
        });
        assert_eq!(
            svc.finish_lookup_counted(id),
            Some((LookupOutcome::NotFound, true)),
            "a peer answered without the record: absence is confirmed"
        );
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
        svc.publish(rec.clone(), 1_000).unwrap();

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
        let mut lookup = Lookup::new(
            ResolveQuery::Name(SpineUri::did([1u8; 32])),
            vec![node(1), node(2)],
        );
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
        let req = node
            .name_resolve_request(peer, 42, ResolveQuery::Capability("web.search".into()))
            .unwrap();
        assert_eq!(req.to, MeshTarget::Agent(peer));
        match &req.payload {
            MeshPayload::NameResolveRequest(r) => assert_eq!(r.request_id, 42),
            other => panic!("expected NameResolveRequest, got {other:?}"),
        }

        let resp = node
            .name_resolve_response(peer, ResolveResponse::empty(42))
            .unwrap();
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
        holder.publish(rec.clone(), 1_000).unwrap();

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

        assert!(
            !lookup.is_done(),
            "a record is not an answer to a node query"
        );
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

    /// Maintenance walks the keyspace once per record it re-offers, so it must
    /// only iterate records this node is actually one of the holders for. A copy
    /// held while K nearer nodes exist is one no lookup will route to; re-offering
    /// it spends a walk telling the rightful holders what they already have.
    #[test]
    fn maintenance_skips_records_this_node_is_not_responsible_for() {
        // A node at one end of the keyspace, with K peers that all sit nearer
        // the record's key than it does.
        let rec = record(4, 1, 1_000, &[]);
        let target = rec.name.key();

        let mut service = NameService::new(NameKey::of(b"far away"));
        service.publish(rec.clone(), 1_000).unwrap();
        assert_eq!(
            service.records_to_maintain(1_000).len(),
            1,
            "with no peers, this node is trivially among the closest"
        );

        for i in 0..K as u8 {
            // Keys derived from the target itself, so each is nearer to it than
            // an unrelated local key is.
            let mut bytes = *target.as_bytes();
            bytes[31] ^= i;
            let nearer = NodeInfo::new(NameKey::from_bytes(bytes), vec![], 100);
            service.add_peer(nearer, agent(i));
        }

        assert!(
            service.records_to_maintain(1_000).is_empty(),
            "K nearer nodes exist, so this copy is not this node's to keep alive"
        );
    }

    /// A node's own publication is unconditional — it must be able to serve the
    /// names it publishes whether or not it happens to sit near their keys — but
    /// an announcement from the network is not. Minting a `did:` name costs one
    /// keygen, so "the signature verified" is no reason to spend memory.
    #[test]
    fn an_announcement_for_a_distant_key_is_declined_but_our_own_publication_is_not() {
        let rec = record(4, 1, 1_000, &[]);
        let target = rec.name.key();

        let mut service = NameService::new(NameKey::of(b"far from that key"));

        // Fill the neighbourhood around the record's key with nearer nodes, so
        // this node is demonstrably not one of the K closest to it.
        for i in 0..K as u8 {
            let mut bytes = *target.as_bytes();
            bytes[31] ^= i;
            service.add_peer(
                NodeInfo::new(NameKey::from_bytes(bytes), vec![], 100),
                agent(i),
            );
        }

        assert_eq!(
            service.accept_announcement(rec.clone(), 1_000).unwrap(),
            spine_name::PutOutcome::Rejected,
            "a record belonging elsewhere in the keyspace is not this node's to hold"
        );
        assert_eq!(service.record_count(), 0);

        // The same record, published by this node rather than announced to it.
        assert_eq!(
            service.publish(rec, 1_000).unwrap(),
            spine_name::PutOutcome::Inserted,
            "a node must be able to serve what it publishes, wherever it sits"
        );
        assert_eq!(service.record_count(), 1);
    }

    /// A record is also placed near the keys of the capabilities it advertises,
    /// so a node near a capability key must accept it even when it is far from
    /// the record's name key, and must index it for capability queries.
    #[test]
    fn an_announcement_is_accepted_near_a_capability_key() {
        let rec = record(4, 1, 1_000, &["web.search"]);
        let target = rec.name.key();

        // The local node sits right next to the capability key, and K peers sit
        // right next to the name key: it is responsible for the former only.
        let mut local = *capability_key("web.search").as_bytes();
        local[31] ^= 1;
        let mut service = NameService::new(NameKey::from_bytes(local));
        for i in 0..K as u8 {
            let mut bytes = *target.as_bytes();
            bytes[31] ^= i;
            service.add_peer(
                NodeInfo::new(NameKey::from_bytes(bytes), vec![], 100),
                agent(i),
            );
        }
        assert!(!service.is_responsible_for(&target));

        assert_eq!(
            service.accept_announcement(rec.clone(), 1_000).unwrap(),
            spine_name::PutOutcome::Inserted,
        );
        assert_eq!(
            service.resolve_locally(&ResolveQuery::Capability("web.search".into()), 1_000),
            Some(LookupOutcome::Providers(vec![rec])),
            "the capability index must answer for the placed record"
        );
    }

    #[test]
    fn capability_placement_is_sorted_deduplicated_and_capped() {
        let caps: Vec<String> = (0..20).map(|i| format!("cap{i:02}")).collect();
        let refs: Vec<&str> = caps.iter().map(|s| s.as_str()).collect();
        let mut rec = record(5, 1, 1_000, &refs);
        rec.capabilities.push("CAP00".into());

        let keys = capability_placement_keys(&rec);
        assert_eq!(keys.len(), MAX_CAPABILITY_KEYS);
        assert_eq!(keys[0], capability_key("cap00"));
        assert_eq!(keys[MAX_CAPABILITY_KEYS - 1], capability_key("cap07"));
    }

    #[test]
    fn the_peer_registry_stays_within_its_bound_and_evicts_the_least_recently_seen() {
        let mut service = NameService::new(NameKey::of(b"registry"));
        let key = |i: usize| NameKey::of(&(i as u64).to_le_bytes());
        for i in 0..MAX_PEERS + 50 {
            service.add_peer(NodeInfo::new(key(i), vec![], 100), AgentId::new());
            assert!(service.addressable_peers() <= MAX_PEERS);
        }
        assert_eq!(service.addressable_peers(), MAX_PEERS);
        // The oldest were evicted, the newest kept.
        assert!(service.agent_for(&key(0)).is_none());
        assert!(service.agent_for(&key(49)).is_none());
        assert!(service.agent_for(&key(MAX_PEERS + 49)).is_some());
        // Evicted peers are gone from the routing table too.
        assert!(service.node_count() <= MAX_PEERS);
    }

    #[test]
    fn refreshing_a_peer_protects_it_from_eviction() {
        let mut service = NameService::new(NameKey::of(b"registry"));
        let key = |i: usize| NameKey::of(&(i as u64).to_le_bytes());
        let first = AgentId::new();
        service.add_peer(NodeInfo::new(key(0), vec![], 100), first);
        for i in 1..MAX_PEERS {
            service.add_peer(NodeInfo::new(key(i), vec![], 100), AgentId::new());
        }
        // Touch the oldest, then overflow by one: the next-oldest goes instead.
        service.add_peer(NodeInfo::new(key(0), vec![], 200), first);
        service.add_peer(NodeInfo::new(key(MAX_PEERS), vec![], 100), AgentId::new());
        assert_eq!(service.agent_for(&key(0)), Some(first));
        assert!(service.agent_for(&key(1)).is_none());
        assert_eq!(service.addressable_peers(), MAX_PEERS);
    }

    #[test]
    fn an_oversized_announcement_is_rejected_even_if_it_verifies() {
        let key = SigningKey::from_bytes(&[9; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name, 1, 1_000).unwrap();
        for i in 0..200 {
            rec = rec.with_capability(format!("cap{i}"));
        }
        rec.sign(&key).unwrap();
        assert!(rec.verify().is_ok());

        let mut service = NameService::new(NameKey::of(b"receiver"));
        assert!(matches!(
            service.accept_announcement(rec, 1_000),
            Err(spine_name::NameError::RecordTooLarge(_))
        ));
        assert_eq!(service.record_count(), 0);
    }

    /// An expired record is not something to re-offer; it is something to drop.
    #[test]
    fn maintenance_ignores_records_that_have_already_expired() {
        let mut service = NameService::new(NameKey::of(&[1]));
        service.publish(record(4, 1, 1_000, &[]), 1_000).unwrap(); // ttl 3600
        assert_eq!(service.records_to_maintain(2_000).len(), 1);
        assert!(service.records_to_maintain(9_000).is_empty());
    }

    /// Re-offering a record cannot extend its life, so a record near expiry is
    /// something to report upward rather than something to fix here.
    #[test]
    fn a_record_close_to_expiry_is_reported_as_lapsing() {
        let mut service = NameService::new(NameKey::of(&[1]));
        let rec = record(4, 1, 1_000, &[]); // ttl 3600
        let name = rec.name.clone();
        service.publish(rec, 1_000).unwrap();

        assert!(service.lapsing(1_000, 900).is_empty(), "not yet");
        assert_eq!(service.lapsing(4_000, 900), vec![name], "600s left");
        assert!(
            service.lapsing(9_000, 900).is_empty(),
            "already expired, so past reporting"
        );
    }

    fn name_response(rec: Option<NameRecord>) -> ResolveResponse {
        ResolveResponse {
            request_id: 1,
            record: rec,
            providers: vec![],
            closer: vec![],
        }
    }

    #[test]
    fn an_old_record_answering_first_cannot_win_the_lookup() {
        let name = record(1, 1, 1_000, &[]).name.clone();
        let mut lookup = Lookup::new(ResolveQuery::Name(name), vec![node(1), node(2), node(3)]);
        assert_eq!(lookup.next_wave().len(), 3);

        // The first peer holds an old, validly signed record.
        assert!(lookup.on_response_from(
            &[node(1).id],
            &name_response(Some(record(1, 1, 1_000, &[])))
        ));
        assert!(!lookup.is_done(), "one answer must not end a name lookup");
        assert!(lookup.outcome().is_none());

        // The second holds the newer one, the third nothing.
        assert!(lookup.on_response_from(
            &[node(2).id],
            &name_response(Some(record(1, 5, 1_000, &[])))
        ));
        assert!(!lookup.is_done(), "two of three is below the quorum");
        assert!(lookup.on_response_from(&[node(3).id], &name_response(None)));
        assert!(lookup.is_done());
        match lookup.outcome().unwrap() {
            LookupOutcome::Found(r) => assert_eq!(r.seq, 5, "the superseding record wins"),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn a_small_mesh_settles_once_everyone_reachable_has_answered() {
        let name = record(1, 1, 1_000, &[]).name.clone();
        // Two candidates, fewer than QUORUM: both must answer, then it settles.
        let mut lookup = Lookup::new(ResolveQuery::Name(name.clone()), vec![node(1), node(2)]);
        lookup.next_wave();
        lookup.on_response(&name_response(Some(record(1, 1, 1_000, &[]))));
        assert!(!lookup.is_done(), "a peer is still outstanding");
        lookup.on_response(&name_response(Some(record(1, 2, 1_000, &[]))));
        assert!(lookup.is_done());
        match lookup.outcome().unwrap() {
            LookupOutcome::Found(r) => assert_eq!(r.seq, 2),
            other => panic!("expected Found, got {other:?}"),
        }

        // A single candidate: availability over strictness.
        let mut lone = Lookup::new(ResolveQuery::Name(name), vec![node(1)]);
        lone.next_wave();
        lone.on_response(&name_response(Some(record(1, 1, 1_000, &[]))));
        assert!(lone.is_done());
        assert!(matches!(lone.outcome(), Some(LookupOutcome::Found(_))));
    }

    #[test]
    fn one_peer_answering_repeatedly_does_not_meet_the_quorum() {
        let name = record(1, 1, 1_000, &[]).name.clone();
        let mut lookup = Lookup::new(ResolveQuery::Name(name), vec![node(1), node(2), node(3)]);
        lookup.next_wave();
        let old = name_response(Some(record(1, 1, 1_000, &[])));
        assert!(lookup.on_response_from(&[node(1).id], &old));
        assert!(
            !lookup.on_response_from(&[node(1).id], &old),
            "already answered"
        );
        assert!(!lookup.on_response_from(&[node(1).id], &old));
        assert!(!lookup.on_response_from(&[node(9).id], &old), "never asked");
        assert_eq!(lookup.answered(), 1);
        assert!(!lookup.is_done());
    }

    #[test]
    fn the_service_ignores_responses_from_peers_it_did_not_ask() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        for seed in 1..=3u8 {
            svc.add_peer(node(seed), agent(seed));
        }
        let rec = record(1, 1, 1_000, &[]);
        let (id, wave) = svc.start_lookup(ResolveQuery::Name(rec.name.clone()));
        assert_eq!(wave.len(), 3);

        let answer = ResolveResponse {
            request_id: id,
            record: Some(rec),
            providers: vec![],
            closer: vec![],
        };
        assert!(svc.on_response_from(agent(9), &answer).is_none());
        assert!(svc.on_response_from(agent(1), &answer).is_some());
        assert!(
            svc.on_response_from(agent(1), &answer).is_none(),
            "a second answer from the same peer does not count"
        );
        assert_eq!(svc.lookup_in_flight(id), 2);
        assert!(svc.lookup_outcome(id).is_none());
    }

    #[test]
    fn an_expired_record_is_not_returned_but_its_peer_still_answered() {
        // Published at 1_000 with a 3600s ttl: expires at 4_600.
        let rec = record(1, 1, 1_000, &[]);
        let mut lookup = Lookup::new(ResolveQuery::Name(rec.name.clone()), vec![node(1)]);
        lookup.set_clock(5_000);
        lookup.next_wave();
        lookup.on_response(&name_response(Some(rec.clone())));
        assert_eq!(lookup.answered(), 1, "the peer answered");
        assert!(lookup.next_wave().is_empty());
        assert_eq!(
            lookup.outcome(),
            Some(LookupOutcome::NotFound),
            "an expired record is not an answer"
        );

        // The same record is accepted while it is still fresh.
        let mut fresh = Lookup::new(ResolveQuery::Name(rec.name.clone()), vec![node(1)]);
        fresh.set_clock(4_000);
        fresh.next_wave();
        fresh.on_response(&name_response(Some(rec)));
        assert!(matches!(fresh.outcome(), Some(LookupOutcome::Found(_))));
    }

    #[test]
    fn an_expired_record_does_not_hide_a_fresh_one_from_another_peer() {
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut stale = record(1, 1, 1_000, &[]).with_ttl(60);
        stale.sign(&key).unwrap(); // expires at 1_060
        let live = record(1, 2, 5_000, &[]);
        let mut lookup = Lookup::new(
            ResolveQuery::Name(live.name.clone()),
            vec![node(1), node(2)],
        );
        lookup.set_clock(5_000);
        lookup.next_wave();
        lookup.on_response(&name_response(Some(stale)));
        assert!(!lookup.is_done());
        lookup.on_response(&name_response(Some(live.clone())));
        assert_eq!(lookup.outcome(), Some(LookupOutcome::Found(Box::new(live))));
    }

    #[test]
    fn the_service_clock_reaches_the_lookup() {
        let mut svc = NameService::new(NameKey::of(b"local"));
        svc.add_node(node(1));
        let rec = record(1, 1, 1_000, &[]);
        let (id, wave) = svc.start_lookup_at(ResolveQuery::Name(rec.name.clone()), 9_000);
        assert_eq!(wave.len(), 1);
        svc.on_response(&ResolveResponse {
            request_id: id,
            record: Some(rec),
            providers: vec![],
            closer: vec![],
        });
        assert_eq!(
            svc.finish_lookup_counted(id),
            Some((LookupOutcome::NotFound, true))
        );
    }

    #[test]
    fn publishing_an_oversized_record_is_refused() {
        let key = SigningKey::from_bytes(&[9; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name, 1, 1_000).unwrap();
        for i in 0..200 {
            rec = rec.with_capability(format!("cap{i}"));
        }
        rec.sign(&key).unwrap();
        assert!(rec.verify().is_ok());

        let mut service = NameService::new(NameKey::of(b"publisher"));
        assert!(matches!(
            service.publish(rec, 1_000),
            Err(spine_name::NameError::RecordTooLarge(_))
        ));
        assert_eq!(service.record_count(), 0);
    }

    #[test]
    fn a_response_carrying_an_oversized_record_is_refused() {
        let key = SigningKey::from_bytes(&[9; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name.clone(), 1, 1_000).unwrap();
        for i in 0..200 {
            rec = rec.with_capability(format!("cap{i}"));
        }
        rec.sign(&key).unwrap();

        let mut lookup = Lookup::new(ResolveQuery::Name(name), vec![node(1)]);
        lookup.next_wave();
        lookup.on_response(&name_response(Some(rec)));
        assert_eq!(lookup.answered(), 1);
        assert!(lookup.next_wave().is_empty());
        assert_eq!(lookup.outcome(), Some(LookupOutcome::NotFound));
    }

    #[test]
    fn excess_endpoints_are_dropped_rather_than_failing_the_message() {
        let mut endpoints: Vec<String> = (0..20).map(|i| format!("10.0.0.{i}:9440")).collect();
        endpoints.insert(0, "x".repeat(MAX_ENDPOINT_BYTES + 1));
        let bounded = bound_endpoints(&endpoints);
        assert_eq!(bounded.len(), MAX_NODE_ENDPOINTS);
        assert!(bounded.iter().all(|e| e.len() <= MAX_ENDPOINT_BYTES));
        assert_eq!(
            bounded[0], "10.0.0.0:9440",
            "the oversized entry was dropped"
        );

        let exact = "y".repeat(MAX_ENDPOINT_BYTES);
        assert_eq!(bound_endpoints(std::slice::from_ref(&exact)), vec![exact]);
    }

    #[test]
    fn excess_referred_nodes_and_their_endpoints_are_dropped() {
        let fat = |seed: u8| {
            KeyspacePeer::new(
                NodeInfo::new(
                    NameKey::of(&[seed]),
                    (0..30).map(|i| format!("h{seed}-{i}:1")).collect(),
                    100,
                ),
                agent(seed),
            )
        };
        let referred: Vec<KeyspacePeer> = (1..=40u8).map(fat).collect();
        let bounded = bound_referrals(&referred);
        assert_eq!(bounded.len(), K);
        assert!(bounded
            .iter()
            .all(|p| p.info.endpoints.len() == MAX_NODE_ENDPOINTS));

        // And the service stores only the bounded form.
        let mut svc = NameService::new(NameKey::of(b"local"));
        svc.add_node(node(100));
        let (id, _) = svc.start_lookup(ResolveQuery::Name(SpineUri::did([1u8; 32])));
        svc.on_response(&ResolveResponse {
            request_id: id,
            record: None,
            providers: vec![],
            closer: referred,
        });
        assert!(svc.node_count() <= K + 1, "at most K referrals are kept");
        assert!(svc.addressable_peers() <= K);
        for stored in svc.routing.all() {
            assert!(stored.endpoints.len() <= MAX_NODE_ENDPOINTS);
        }
    }

    /// A record already held at an equal or newer version must not be treated as
    /// news, or two nodes holding different versions announce at each other
    /// without end.
    #[test]
    fn re_offering_the_same_record_is_reported_as_superseded() {
        let mut service = NameService::new(NameKey::of(&[1]));
        let rec = record(4, 1, 1_000, &[]);
        assert_eq!(
            service.publish(rec.clone(), 1_000).unwrap(),
            spine_name::PutOutcome::Inserted
        );
        assert_eq!(
            service.publish(rec, 1_000).unwrap(),
            spine_name::PutOutcome::Superseded
        );
    }
}
