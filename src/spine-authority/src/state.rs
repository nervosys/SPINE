//! The state verification needs but a pure crate cannot hold: what has been
//! revoked, and what has been used.
//!
//! Both are traits the caller implements over its own storage. Each has a
//! default implementation that fails closed, and a plain in-memory
//! implementation for tests and single-process callers.

use std::collections::{BTreeMap, BTreeSet};

use crate::grant::{GrantId, PublicKey};
use crate::verification::Decision;

/// Length, in seconds, of the rolling window the `rate` limit counts calls in.
pub const RATE_WINDOW_SECS: u64 = 60;

/// What a grant has been charged so far, as far as a ledger knows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Spent {
    /// Invocations charged.
    pub calls: u64,
    /// Declared cost charged.
    pub cost: u64,
    /// Declared bytes charged.
    pub bytes: u64,
    /// Invocations charged within the last [`RATE_WINDOW_SECS`] seconds.
    pub recent_calls: u64,
}

/// What a verifier knows about revocation (step 8).
pub trait RevocationView {
    /// Whether this grant id is known to be revoked. An ancestor's revocation
    /// is found by asking about each grant of the chain in turn; the view does
    /// not need to know the chain.
    fn is_revoked(&self, id: &GrantId) -> bool;

    /// How old, in seconds at time `now`, the revocation data behind this view
    /// is. `None` means there is no revocation data at all. Verification
    /// compares this with the staleness bound for the invocation's effect class.
    fn data_age_secs(&self, now: u64) -> Option<u64>;
}

/// What a performing party remembers (steps 9 and 11).
pub trait UsageLedger {
    /// Whether this invoker has already used this nonce.
    fn nonce_seen(&self, invoker: &PublicKey, nonce: &[u8; 16]) -> bool;

    /// What the grant has been charged, at time `now`. `None` means the
    /// ledger cannot say; verification then refuses any chain that contains a
    /// grant with a limit.
    fn spent(&self, id: &GrantId, now: u64) -> Option<Spent>;
}

/// The revocation view that knows nothing: no data, so every verification
/// fails at step 8. This is the fail-closed default.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRevocationData;

impl RevocationView for NoRevocationData {
    fn is_revoked(&self, _id: &GrantId) -> bool {
        false
    }

    fn data_age_secs(&self, _now: u64) -> Option<u64> {
        None
    }
}

/// The ledger that remembers nothing: every nonce counts as already seen, so
/// every verification fails at step 9. This is the fail-closed default.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoLedger;

impl UsageLedger for NoLedger {
    fn nonce_seen(&self, _invoker: &PublicKey, _nonce: &[u8; 16]) -> bool {
        true
    }

    fn spent(&self, _id: &GrantId, _now: u64) -> Option<Spent> {
        None
    }
}

/// A set of revoked ids and the time the set was last known to be complete.
#[derive(Debug, Clone, Default)]
pub struct RevocationSet {
    /// The revoked grant ids.
    pub revoked: BTreeSet<GrantId>,
    /// The Unix time (seconds) as of which `revoked` is complete. `None`:
    /// no data.
    pub as_of: Option<u64>,
}

impl RevocationSet {
    /// An empty set that claims to be complete as of `as_of`.
    pub fn current(as_of: u64) -> RevocationSet {
        RevocationSet {
            revoked: BTreeSet::new(),
            as_of: Some(as_of),
        }
    }

    /// Adds an id.
    pub fn revoke(&mut self, id: GrantId) {
        self.revoked.insert(id);
    }
}

impl RevocationView for RevocationSet {
    fn is_revoked(&self, id: &GrantId) -> bool {
        self.revoked.contains(id)
    }

    fn data_age_secs(&self, now: u64) -> Option<u64> {
        self.as_of.map(|as_of| now.saturating_sub(as_of))
    }
}

#[derive(Debug, Clone, Default)]
struct Usage {
    calls: u64,
    cost: u64,
    bytes: u64,
    times: Vec<u64>,
}

/// An in-memory ledger. It grows without bound and is not shared between
/// processes: it is for tests and for a single-process performing party.
///
/// [`crate::verify`] only reads the ledger. After it accepts an invocation the
/// caller must call [`MemoryLedger::charge`] (or do the same against its own
/// store) in the same atomic step that performs the operation, or two
/// concurrent copies of one invocation could both pass.
#[derive(Debug, Clone, Default)]
pub struct MemoryLedger {
    seen: BTreeSet<(PublicKey, [u8; 16])>,
    usage: BTreeMap<GrantId, Usage>,
}

impl MemoryLedger {
    /// An empty ledger.
    pub fn new() -> MemoryLedger {
        MemoryLedger::default()
    }

    /// Records an accepted invocation: its nonce, and one call, its cost and
    /// its bytes against every grant of the chain.
    pub fn charge(&mut self, decision: &Decision) {
        self.seen.insert((decision.invoker, decision.nonce));
        for id in &decision.chain_ids {
            let entry = self.usage.entry(*id).or_default();
            entry.calls = entry.calls.saturating_add(1);
            entry.cost = entry.cost.saturating_add(decision.cost);
            entry.bytes = entry.bytes.saturating_add(decision.bytes);
            entry.times.push(decision.now);
        }
    }

    /// Sets the totals of a grant, as if that much had already been charged.
    pub fn preload(&mut self, id: GrantId, calls: u64, cost: u64, bytes: u64) {
        let entry = self.usage.entry(id).or_default();
        entry.calls = calls;
        entry.cost = cost;
        entry.bytes = bytes;
    }

    /// Records that a call was charged to the grant at `time`.
    pub fn record_call_at(&mut self, id: GrantId, time: u64) {
        let entry = self.usage.entry(id).or_default();
        entry.calls = entry.calls.saturating_add(1);
        entry.times.push(time);
    }
}

impl UsageLedger for MemoryLedger {
    fn nonce_seen(&self, invoker: &PublicKey, nonce: &[u8; 16]) -> bool {
        self.seen.contains(&(*invoker, *nonce))
    }

    fn spent(&self, id: &GrantId, now: u64) -> Option<Spent> {
        let spent = match self.usage.get(id) {
            Some(usage) => Spent {
                calls: usage.calls,
                cost: usage.cost,
                bytes: usage.bytes,
                recent_calls: usage
                    .times
                    .iter()
                    .filter(|t| t.saturating_add(RATE_WINDOW_SECS) > now)
                    .count() as u64,
            },
            None => Spent::default(),
        };
        Some(spent)
    }
}
