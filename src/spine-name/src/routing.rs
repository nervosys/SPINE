//! Kademlia-style k-bucket routing over the mesh's existing peers.
//!
//! SPINE's mesh already gossips peers, routes multi-hop, and dedups by message
//! id. What it lacked was a *keyspace* to route in: peers were known by socket
//! address, so finding anything meant already knowing where it was. Bucketing
//! peers by XOR distance turns that same peer table into a structure where each
//! hop at least halves the remaining distance to any target.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::key::NameKey;

/// Default bucket width. 20 is Kademlia's k: large enough that a bucket is
/// unlikely to be emptied by simultaneous churn, small enough to keep lookups
/// cheap.
pub const DEFAULT_K: usize = 20;

/// A node in the keyspace, as known to the routing table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Position in the shared keyspace — for a SPINE agent this is its Ed25519
    /// public key, so a node id is self-certifying just like a `did:` name.
    pub id: NameKey,
    /// Transport addresses to reach it (`host:port`, `ws://…`).
    pub endpoints: Vec<String>,
    /// Monotonic freshness counter, in seconds since the epoch. The caller
    /// supplies the clock so the table stays testable and `no_std`-friendly.
    pub last_seen: u64,
}

impl NodeInfo {
    pub fn new(id: NameKey, endpoints: Vec<String>, last_seen: u64) -> Self {
        Self {
            id,
            endpoints,
            last_seen,
        }
    }
}

/// A routing table of k-buckets indexed by shared-prefix length with the local
/// node.
#[derive(Debug, Clone)]
pub struct RoutingTable {
    local: NameKey,
    k: usize,
    /// Sparse: only buckets that have ever held a node are allocated. With 256
    /// possible buckets and realistic network sizes, all but ~log2(n) stay empty.
    buckets: HashMap<usize, Vec<NodeInfo>>,
    /// Nodes this node has itself found unreachable. Only these may be
    /// displaced from a full bucket.
    failed: HashSet<NameKey>,
}

impl RoutingTable {
    /// A table for `local` with the default bucket width.
    pub fn new(local: NameKey) -> Self {
        Self::with_k(local, DEFAULT_K)
    }

    /// A table with an explicit bucket width.
    pub fn with_k(local: NameKey, k: usize) -> Self {
        Self {
            local,
            k: k.max(1),
            buckets: HashMap::new(),
            failed: HashSet::new(),
        }
    }

    /// The local node's key.
    pub fn local(&self) -> &NameKey {
        &self.local
    }

    /// Total nodes known.
    pub fn len(&self) -> usize {
        self.buckets.values().map(|b| b.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Learn of a node by hearsay: a peer referred it to us.
    ///
    /// Returns `false` if the node was not stored: it is the local node, or its
    /// bucket is full of contacts this node has no reason to distrust.
    ///
    /// Nothing about a referral is trusted, `last_seen` least of all: the
    /// referring peer chose it, so using it to rank contacts would let an
    /// attacker flood a bucket with "fresher" sybils and evict honest peers.
    /// Instead this follows Kademlia's preference for long-lived contacts:
    ///
    /// * a node already in the table is left exactly as it is (a referral can
    ///   neither redirect its endpoints nor advance its `last_seen`; only
    ///   [`RoutingTable::insert_observed`] does that);
    /// * a new node takes a free slot if the bucket has one;
    /// * in a full bucket it replaces an entry only if this node has itself
    ///   marked that entry unreachable ([`RoutingTable::mark_unreachable`]);
    ///   otherwise it is dropped.
    pub fn insert(&mut self, node: NodeInfo) -> bool {
        let Some(index) = self.local.bucket_index(&node.id) else {
            return false; // never bucket ourselves
        };
        if self
            .buckets
            .get(&index)
            .is_some_and(|b| b.iter().any(|n| n.id == node.id))
        {
            return true;
        }
        self.insert_new(index, node)
    }

    /// Record a node this node has itself just heard from or connected to.
    ///
    /// Unlike [`RoutingTable::insert`], `now` is the *local* clock and replaces
    /// whatever `last_seen` the `node` carries, so a peer cannot choose how
    /// fresh it looks. A known node has its endpoints and `last_seen`
    /// refreshed and any unreachable mark cleared; a new node is placed by the
    /// same rules as a referral.
    pub fn insert_observed(&mut self, mut node: NodeInfo, now: u64) -> bool {
        let Some(index) = self.local.bucket_index(&node.id) else {
            return false;
        };
        node.last_seen = now;
        let id = node.id;
        if let Some(existing) = self
            .buckets
            .get_mut(&index)
            .and_then(|b| b.iter_mut().find(|n| n.id == id))
        {
            *existing = node;
            self.failed.remove(&id);
            return true;
        }
        self.insert_new(index, node)
    }

    /// Place a node not currently in its bucket.
    fn insert_new(&mut self, index: usize, node: NodeInfo) -> bool {
        let bucket = self.buckets.entry(index).or_default();
        if bucket.len() < self.k {
            bucket.push(node);
            return true;
        }
        let failed = &self.failed;
        match bucket.iter().position(|n| failed.contains(&n.id)) {
            Some(i) => {
                let old = std::mem::replace(&mut bucket[i], node);
                self.failed.remove(&old.id);
                true
            }
            None => false,
        }
    }

    /// Mark a known node as unreachable, e.g. after a lookup timeout. The entry
    /// stays (and is still served) until a newcomer needs its slot in a full
    /// bucket, or until [`RoutingTable::insert_observed`] hears from it again.
    /// Returns `false` if the node is not in the table.
    pub fn mark_unreachable(&mut self, id: &NameKey) -> bool {
        if self.contains(id) {
            self.failed.insert(*id);
            true
        } else {
            false
        }
    }

    /// Forget a node.
    pub fn remove(&mut self, id: &NameKey) -> bool {
        let Some(index) = self.local.bucket_index(id) else {
            return false;
        };
        let Some(bucket) = self.buckets.get_mut(&index) else {
            return false;
        };
        let before = bucket.len();
        bucket.retain(|n| &n.id != id);
        let removed = before != bucket.len();
        self.failed.remove(id);
        removed
    }

    /// Whether a node is known.
    pub fn contains(&self, id: &NameKey) -> bool {
        self.local
            .bucket_index(id)
            .and_then(|i| self.buckets.get(&i))
            .is_some_and(|b| b.iter().any(|n| &n.id == id))
    }

    /// The `n` known nodes closest to `target`, nearest first — the set a
    /// lookup queries next.
    pub fn closest(&self, target: &NameKey, n: usize) -> Vec<NodeInfo> {
        let mut all: Vec<&NodeInfo> = self.buckets.values().flatten().collect();
        all.sort_by_key(|node| node.id.distance(target));
        all.into_iter().take(n).cloned().collect()
    }

    /// Every known node.
    pub fn all(&self) -> Vec<NodeInfo> {
        self.buckets.values().flatten().cloned().collect()
    }

    /// Drop nodes not seen since `cutoff`, returning how many were pruned.
    pub fn prune_stale(&mut self, cutoff: u64) -> usize {
        let mut removed = 0;
        for bucket in self.buckets.values_mut() {
            let before = bucket.len();
            bucket.retain(|n| n.last_seen >= cutoff);
            removed += before - bucket.len();
        }
        self.buckets.retain(|_, b| !b.is_empty());
        let live: HashSet<NameKey> = self.buckets.values().flatten().map(|n| n.id).collect();
        self.failed.retain(|id| live.contains(id));
        removed
    }

    /// Occupancy per non-empty bucket, for diagnostics.
    pub fn bucket_sizes(&self) -> Vec<(usize, usize)> {
        let mut sizes: Vec<(usize, usize)> =
            self.buckets.iter().map(|(i, b)| (*i, b.len())).collect();
        sizes.sort();
        sizes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(seed: u8, seen: u64) -> NodeInfo {
        NodeInfo::new(
            NameKey::of(&[seed]),
            vec![format!("127.0.0.1:{}", 9000 + seed as u16)],
            seen,
        )
    }

    fn table() -> RoutingTable {
        RoutingTable::new(NameKey::of(b"local"))
    }

    #[test]
    fn inserts_and_finds_nodes() {
        let mut t = table();
        assert!(t.insert(node(1, 100)));
        assert!(t.insert(node(2, 100)));
        assert_eq!(t.len(), 2);
        assert!(t.contains(&NameKey::of(&[1])));
        assert!(!t.contains(&NameKey::of(&[99])));
    }

    #[test]
    fn never_buckets_the_local_node() {
        let local = NameKey::of(b"local");
        let mut t = RoutingTable::new(local);
        assert!(!t.insert(NodeInfo::new(local, vec![], 100)));
        assert!(t.is_empty());
    }

    #[test]
    fn reinserting_refreshes_rather_than_duplicates() {
        let mut t = table();
        t.insert(node(1, 100));
        t.insert_observed(
            NodeInfo::new(NameKey::of(&[1]), vec!["10.0.0.1:1".into()], 0),
            200,
        );
        assert_eq!(t.len(), 1);
        let found = t.closest(&NameKey::of(&[1]), 1).pop().unwrap();
        assert_eq!(found.last_seen, 200, "stamped from the local clock");
        assert_eq!(found.endpoints, vec!["10.0.0.1:1".to_string()]);
    }

    #[test]
    fn a_stale_sighting_does_not_overwrite_a_fresher_one() {
        let mut t = table();
        t.insert(node(1, 500));
        t.insert(NodeInfo::new(NameKey::of(&[1]), vec!["old".into()], 100));
        let found = t.closest(&NameKey::of(&[1]), 1).pop().unwrap();
        assert_eq!(found.last_seen, 500);
    }

    #[test]
    fn closest_ranks_by_xor_distance() {
        let mut t = table();
        for seed in 1..=20u8 {
            t.insert(node(seed, 100));
        }
        let target = NameKey::of(&[7]);
        let closest = t.closest(&target, 3);
        assert_eq!(closest[0].id, target, "the exact match must rank first");
        // Strictly increasing distance.
        for pair in closest.windows(2) {
            assert!(pair[0].id.distance(&target) <= pair[1].id.distance(&target));
        }
    }

    #[test]
    fn closest_caps_at_the_requested_count_and_at_what_is_known() {
        let mut t = table();
        for seed in 1..=5u8 {
            t.insert(node(seed, 100));
        }
        assert_eq!(t.closest(&NameKey::of(b"x"), 3).len(), 3);
        assert_eq!(t.closest(&NameKey::of(b"x"), 50).len(), 5);
    }

    /// Three keys that share bucket 255 of an all-zero local key.
    fn same_bucket_keys() -> (NameKey, NameKey, NameKey) {
        let mut a = [0u8; 32];
        a[0] = 0b1000_0000;
        let mut b = [0u8; 32];
        b[0] = 0b1100_0000;
        let mut c = [0u8; 32];
        c[0] = 0b1010_0000;
        (
            NameKey::from_bytes(a),
            NameKey::from_bytes(b),
            NameKey::from_bytes(c),
        )
    }

    #[test]
    fn a_full_bucket_keeps_its_entries_against_a_newcomer_claiming_a_later_last_seen() {
        // k=1 makes the rule observable in one bucket.
        let local = NameKey::from_bytes([0u8; 32]);
        let mut t = RoutingTable::with_k(local, 1);
        let (a, b, _) = same_bucket_keys();
        assert_eq!(local.bucket_index(&a), Some(255));
        assert_eq!(local.bucket_index(&b), Some(255));

        assert!(t.insert(NodeInfo::new(a, vec![], 500)));
        assert!(!t.insert(NodeInfo::new(b, vec![], 100)));
        assert!(!t.insert(NodeInfo::new(b, vec![], u64::MAX)));
        assert!(!t.insert_observed(NodeInfo::new(b, vec![], u64::MAX), 900));
        assert!(t.contains(&a));
        assert!(!t.contains(&b));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn a_referral_cannot_redirect_or_refresh_a_known_node() {
        let mut t = table();
        t.insert(node(1, 100));
        assert!(t.insert(NodeInfo::new(
            NameKey::of(&[1]),
            vec!["evil".into()],
            u64::MAX
        )));
        let found = t.closest(&NameKey::of(&[1]), 1).pop().unwrap();
        assert_eq!(found.last_seen, 100);
        assert_ne!(found.endpoints, vec!["evil".to_string()]);
    }

    #[test]
    fn an_entry_marked_unreachable_is_replaced_by_a_newcomer() {
        let local = NameKey::from_bytes([0u8; 32]);
        let mut t = RoutingTable::with_k(local, 1);
        let (a, b, c) = same_bucket_keys();

        assert!(t.insert(NodeInfo::new(a, vec![], 500)));
        assert!(!t.mark_unreachable(&b), "unknown nodes cannot be marked");
        assert!(t.mark_unreachable(&a));
        assert!(t.insert(NodeInfo::new(b, vec![], 1)));
        assert!(t.contains(&b));
        assert!(!t.contains(&a));
        assert_eq!(t.len(), 1);

        // The replacement was never marked, so it is now protected.
        assert!(!t.insert(NodeInfo::new(c, vec![], u64::MAX)));
        assert!(t.contains(&b));
    }

    #[test]
    fn hearing_from_a_node_again_clears_its_unreachable_mark() {
        let local = NameKey::from_bytes([0u8; 32]);
        let mut t = RoutingTable::with_k(local, 1);
        let (a, b, _) = same_bucket_keys();
        t.insert(NodeInfo::new(a, vec![], 10));
        t.mark_unreachable(&a);
        assert!(t.insert_observed(NodeInfo::new(a, vec![], 0), 50));
        assert!(!t.insert(NodeInfo::new(b, vec![], 99)));
        assert!(t.contains(&a));
    }

    #[test]
    fn removes_and_prunes() {
        let mut t = table();
        t.insert(node(1, 100));
        t.insert(node(2, 900));
        assert!(t.remove(&NameKey::of(&[1])));
        assert!(!t.remove(&NameKey::of(&[1])));
        assert_eq!(t.len(), 1);

        t.insert(node(3, 100));
        assert_eq!(t.prune_stale(500), 1);
        assert_eq!(t.len(), 1);
        assert!(t.contains(&NameKey::of(&[2])));
    }

    #[test]
    fn nodes_spread_across_buckets_by_prefix() {
        let mut t = table();
        for seed in 1..=40u8 {
            t.insert(node(seed, 100));
        }
        // With hashed ids, occupancy should not collapse into a single bucket.
        assert!(
            t.bucket_sizes().len() > 1,
            "expected multiple buckets, got {:?}",
            t.bucket_sizes()
        );
        assert_eq!(
            t.bucket_sizes().iter().map(|(_, n)| n).sum::<usize>(),
            t.len()
        );
    }

    #[test]
    fn lookup_converges_because_each_hop_halves_the_distance() {
        // The property that makes O(log n) resolution work: from any node, the
        // closest known peer to a target is strictly nearer than the node itself.
        let mut t = table();
        for seed in 0..=255u8 {
            t.insert(node(seed, 100));
        }
        let target = NameKey::of(b"some name");
        let best = t.closest(&target, 1).pop().unwrap();
        let mut improved = 0;
        for candidate in t.all() {
            if candidate.id.distance(&target) < best.id.distance(&target) {
                improved += 1;
            }
        }
        assert_eq!(improved, 0, "closest() must return the true minimum");
    }
}
