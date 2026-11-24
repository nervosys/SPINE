//! The state verification needs but a pure crate cannot hold: what has been
//! revoked, and what has been used.
//!
//! Both are traits the caller implements over its own storage. Each has a
//! default implementation that fails closed, and a plain in-memory
//! implementation for tests and single-process callers. For revocation that
//! is [`RevocationSet`] (ids the caller already trusts) and, for signed
//! records with a signed head, [`crate::RevocationFeed`]. For usage it is
//! [`MemoryLedger`].

use std::collections::{BTreeMap, BTreeSet};

use crate::grant::{Chain, GrantId, Limits, PublicKey};
use crate::verification::{over, Decision};

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

    /// Whether any grant of `chain` is revoked. `ids` are the ids of
    /// `chain.grants`, in the same order. [`crate::verify`] calls this, not
    /// [`RevocationView::is_revoked`], so that a view holding signed records
    /// can check that each record's signer had the right to revoke the grant
    /// it names, which depends on the chain ([`crate::Revocation::is_authorized_by`]).
    ///
    /// The default asks [`RevocationView::is_revoked`] about every id.
    fn chain_revoked(&self, _chain: &Chain, ids: &[GrantId]) -> bool {
        ids.iter().any(|id| self.is_revoked(id))
    }

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

/// Bounds of a [`MemoryLedger`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerConfig {
    /// Most nonces remembered at once. Default 65536.
    pub max_nonces: usize,
    /// Most grants with a usage record at once. Default 16384.
    pub max_grants: usize,
}

impl Default for LedgerConfig {
    fn default() -> LedgerConfig {
        LedgerConfig {
            max_nonces: 65_536,
            max_grants: 16_384,
        }
    }
}

/// Why [`MemoryLedger::charge`] charged nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ChargeError {
    /// The invoker has already used this nonce.
    #[error("the nonce was already used")]
    NonceReplayed,
    /// Charging would put a grant of the chain over a limit.
    #[error("a limit of the chain would be exceeded")]
    LimitExhausted,
    /// The ledger is at a bound and nothing in it may be evicted yet. Charging
    /// is refused rather than forgetting a nonce or a total that still counts.
    #[error("the ledger is full of records that are still needed")]
    LedgerFull,
    /// The decision's lists differ in length, or one grant appears twice. A
    /// decision made by [`crate::verify`] is never like this.
    #[error("the decision is not well formed")]
    MalformedDecision,
}

type NonceKey = (PublicKey, [u8; 16]);
type HorizonKey = (u64, PublicKey, [u8; 16]);

#[derive(Debug, Clone)]
struct Usage {
    calls: u64,
    cost: u64,
    bytes: u64,
    /// The grant's `exp`: from then on step 7 refuses the grant, so the
    /// record is never read again and may be dropped.
    expires: u64,
    /// Calls per Unix second, only for seconds still inside the rate window
    /// as of the last charge. At most [`RATE_WINDOW_SECS`] entries when time
    /// moves forward.
    window: BTreeMap<u64, u64>,
}

impl Usage {
    fn new(expires: u64) -> Usage {
        Usage {
            calls: 0,
            cost: 0,
            bytes: 0,
            expires,
            window: BTreeMap::new(),
        }
    }

    fn recent(&self, now: u64) -> u64 {
        self.window
            .iter()
            .filter(|(second, _)| second.saturating_add(RATE_WINDOW_SECS) > now)
            .fold(0u64, |total, (_, count)| total.saturating_add(*count))
    }

    fn spent(&self, now: u64) -> Spent {
        Spent {
            calls: self.calls,
            cost: self.cost,
            bytes: self.bytes,
            recent_calls: self.recent(now),
        }
    }

    /// This record after one more charge of `decision`.
    fn charged(&self, decision: &Decision, expires: u64) -> Usage {
        let now = decision.now;
        let mut next = self.clone();
        next.expires = next.expires.max(expires);
        next.calls = next.calls.saturating_add(1);
        next.cost = next.cost.saturating_add(decision.cost);
        next.bytes = next.bytes.saturating_add(decision.bytes);
        next.window
            .retain(|second, _| second.saturating_add(RATE_WINDOW_SECS) > now);
        let slot = next.window.entry(now).or_insert(0);
        *slot = slot.saturating_add(1);
        next
    }

    fn to_record(&self, id: GrantId) -> UsageRecord {
        UsageRecord {
            id,
            calls: self.calls,
            cost: self.cost,
            bytes: self.bytes,
            expires: self.expires,
            window: self.window.iter().map(|(s, c)| (*s, *c)).collect(),
        }
    }

    fn from_record(record: &UsageRecord) -> Usage {
        let mut window: BTreeMap<u64, u64> = BTreeMap::new();
        for (second, count) in &record.window {
            let slot = window.entry(*second).or_insert(0);
            *slot = slot.saturating_add(*count);
        }
        Usage {
            calls: record.calls,
            cost: record.cost,
            bytes: record.bytes,
            expires: record.expires,
            window,
        }
    }
}

fn has_duplicate(ids: &[GrantId]) -> bool {
    ids.iter()
        .enumerate()
        .any(|(index, id)| ids[index + 1..].contains(id))
}

/// A nonce a ledger remembers, and until when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonceRecord {
    /// The acting key.
    pub invoker: PublicKey,
    /// The invocation's nonce.
    pub nonce: [u8; 16],
    /// Remembered until this time (`Decision::nonce_horizon`); dropped only
    /// once the verifier's time is past it.
    pub horizon: u64,
}

/// Everything a [`MemoryLedger`] holds about one grant: the totals, the grant's
/// `exp`, and the calls per Unix second still inside the rate window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRecord {
    /// The grant.
    pub id: GrantId,
    /// Invocations charged.
    pub calls: u64,
    /// Declared cost charged.
    pub cost: u64,
    /// Declared bytes charged.
    pub bytes: u64,
    /// The grant's `exp`: the record may be dropped once the time is past it.
    pub expires: u64,
    /// `(second, calls)` pairs, in increasing order of second.
    pub window: Vec<(u64, u64)>,
}

/// What [`MemoryLedger::plan_charge`] decided: the records a charge will
/// leave behind. Nothing has been applied yet.
///
/// A caller with its own store writes these rows first and calls
/// [`MemoryLedger::commit`] only if the write succeeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChargePlan {
    /// The nonce to remember.
    pub nonce: NonceRecord,
    /// The new value of the record of every limited grant of the chain, in the
    /// order of the chain.
    pub usage: Vec<UsageRecord>,
}

/// What [`MemoryLedger::expire`] dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expired {
    /// Invokers and nonces whose horizon had passed.
    pub nonces: Vec<(PublicKey, [u8; 16])>,
    /// Grants that had expired.
    pub grants: Vec<GrantId>,
}

/// An in-memory ledger for a single performing party in one process.
///
/// [`crate::verify`] only reads the ledger. After it accepts an invocation the
/// caller must call [`MemoryLedger::charge`] (or do the same against its own
/// store) in the same atomic step that performs the operation, or two
/// concurrent copies of one invocation could both pass. `charge` is atomic
/// within the ledger: it charges every grant of the chain and records the
/// nonce, or it changes nothing.
///
/// # What a charge does
///
/// 1. Refuses a malformed [`Decision`].
/// 2. Drops nonces whose replay horizon has passed (see below).
/// 3. Refuses a nonce already recorded ([`ChargeError::NonceReplayed`]).
/// 4. Checks, for every grant of the chain that has a limit, that one more
///    call and the decision's cost and bytes stay within `calls`, `cost`,
///    `bytes` and `rate` ([`ChargeError::LimitExhausted`]). Nothing has been
///    changed so far.
/// 5. Checks the bounds ([`ChargeError::LedgerFull`]).
/// 6. Only then records the nonce and charges every grant.
///
/// A grant with no limit is not tracked: [`crate::verify`] does not consult
/// the ledger for it.
///
/// # Bounds and eviction
///
/// Memory is bounded by [`LedgerConfig`]: at most `max_nonces` nonces and
/// `max_grants` usage records, and per record at most one counter per second of
/// the rolling window (60 under a monotone clock), whatever the `rate` limit.
///
/// A nonce is kept until `Decision::nonce_horizon`, the last time at which an
/// invocation accepted now could still pass the freshness check of step 9
/// (accepted at `now`, so dated at most `skew` ahead and at most
/// `max_invocation_age` behind: `now + skew + max_invocation_age`).
/// A nonce is evicted only after its horizon, so an eviction never lets a
/// replay through that step 9 would have accepted. When the ledger is full of
/// nonces whose horizon has not passed it refuses to charge
/// ([`ChargeError::LedgerFull`]): it fails closed instead of forgetting.
///
/// A usage record is dropped only once its grant has expired, since step 7
/// refuses the grant from then on. When the usage table is full of live
/// grants the ledger likewise refuses.
///
/// The time passed in must not run backwards. The ledger trusts the verifier's
/// clock; a clock that jumps forward makes it forget nonces early.
#[derive(Debug, Clone, Default)]
pub struct MemoryLedger {
    config: LedgerConfig,
    nonces: BTreeMap<NonceKey, u64>,
    horizons: BTreeSet<HorizonKey>,
    usage: BTreeMap<GrantId, Usage>,
}

impl MemoryLedger {
    /// An empty ledger with the default bounds.
    pub fn new() -> MemoryLedger {
        MemoryLedger::default()
    }

    /// An empty ledger with these bounds.
    pub fn with_config(config: LedgerConfig) -> MemoryLedger {
        MemoryLedger {
            config,
            ..MemoryLedger::default()
        }
    }

    /// Number of nonces remembered.
    pub fn nonce_count(&self) -> usize {
        self.nonces.len()
    }

    /// Number of grants with a usage record.
    pub fn grant_count(&self) -> usize {
        self.usage.len()
    }

    fn evict_nonces(&mut self, now: u64) {
        while let Some(first) = self.horizons.iter().next().copied() {
            if first.0 >= now {
                break;
            }
            self.horizons.remove(&first);
            self.nonces.remove(&(first.1, first.2));
        }
    }

    fn evict_usage(&mut self, now: u64) {
        self.usage.retain(|_, usage| usage.expires > now);
    }

    /// Records an accepted invocation: its nonce, and one call, its cost and
    /// its bytes against every grant of the chain that has a limit. Atomic:
    /// on an error nothing was recorded. See the type documentation.
    pub fn charge(&mut self, decision: &Decision) -> Result<(), ChargeError> {
        let plan = self.plan_charge(decision)?;
        self.commit(plan);
        Ok(())
    }

    /// Runs every check of [`MemoryLedger::charge`] and returns the records the
    /// charge would leave, without recording them. Nonces past their horizon
    /// are dropped as a side effect, as in `charge`; nothing that still counts
    /// is changed.
    ///
    /// For a caller that must write the records to its own store before the
    /// ledger moves: write [`ChargePlan::nonce`] and [`ChargePlan::usage`],
    /// then call [`MemoryLedger::commit`] with the plan, with no other call on
    /// this ledger in between. If the write fails, drop the plan; the ledger
    /// is as it was.
    pub fn plan_charge(&mut self, decision: &Decision) -> Result<ChargePlan, ChargeError> {
        let count = decision.chain_ids.len();
        if decision.chain_limits.len() != count
            || decision.chain_expiries.len() != count
            || has_duplicate(decision.chain_ids.as_slice())
        {
            return Err(ChargeError::MalformedDecision);
        }
        let now = decision.now;
        self.evict_nonces(now);
        let key: NonceKey = (decision.invoker, decision.nonce);
        if self.nonces.contains_key(&key) {
            return Err(ChargeError::NonceReplayed);
        }

        let mut new_records = 0usize;
        for (id, limits) in decision.chain_ids.iter().zip(decision.chain_limits.iter()) {
            if !limits.any() {
                continue;
            }
            let spent = match self.usage.get(id) {
                Some(usage) => usage.spent(now),
                None => {
                    new_records += 1;
                    Spent::default()
                }
            };
            if exceeds(&spent, limits, decision) {
                return Err(ChargeError::LimitExhausted);
            }
        }

        if self.nonces.len() >= self.config.max_nonces {
            return Err(ChargeError::LedgerFull);
        }
        if self.usage.len().saturating_add(new_records) > self.config.max_grants {
            self.evict_usage(now);
            if self.usage.len().saturating_add(new_records) > self.config.max_grants {
                return Err(ChargeError::LedgerFull);
            }
        }

        // Every check has passed; nothing below can fail.
        let mut usage = Vec::new();
        let grants = decision
            .chain_ids
            .iter()
            .zip(decision.chain_limits.iter())
            .zip(decision.chain_expiries.iter());
        for ((id, limits), expires) in grants {
            if !limits.any() {
                continue;
            }
            let base = match self.usage.get(id) {
                Some(existing) => existing.clone(),
                None => Usage::new(*expires),
            };
            usage.push(base.charged(decision, *expires).to_record(*id));
        }
        Ok(ChargePlan {
            nonce: NonceRecord {
                invoker: decision.invoker,
                nonce: decision.nonce,
                horizon: decision.nonce_horizon,
            },
            usage,
        })
    }

    /// Applies a plan from [`MemoryLedger::plan_charge`]. Cannot fail. The
    /// plan must come from this ledger, with no other change in between.
    pub fn commit(&mut self, plan: ChargePlan) {
        self.restore_nonce(plan.nonce);
        for record in &plan.usage {
            self.usage.insert(record.id, Usage::from_record(record));
        }
    }

    /// Drops what has expired at `now`: nonces whose horizon has passed and
    /// usage records of grants whose `exp` has passed, and says what it
    /// dropped. `charge` does the same for nonces, and for usage records only
    /// when the table is full; a caller that mirrors the ledger in a store
    /// uses this to learn which rows to delete.
    pub fn expire(&mut self, now: u64) -> Expired {
        let mut expired = Expired::default();
        while let Some(first) = self.horizons.iter().next().copied() {
            if first.0 >= now {
                break;
            }
            self.horizons.remove(&first);
            self.nonces.remove(&(first.1, first.2));
            expired.nonces.push((first.1, first.2));
        }
        expired.grants = self
            .usage
            .iter()
            .filter(|(_, usage)| usage.expires <= now)
            .map(|(id, _)| *id)
            .collect();
        self.evict_usage(now);
        expired
    }

    /// Puts a nonce back, for a ledger rebuilt from a store. Remembering it
    /// again is always the safe direction. Bounds are not checked here, so a
    /// ledger restored over its bounds refuses to charge (`LedgerFull`) until
    /// records expire.
    pub fn restore_nonce(&mut self, record: NonceRecord) {
        let key: NonceKey = (record.invoker, record.nonce);
        if let Some(old) = self.nonces.insert(key, record.horizon) {
            self.horizons.remove(&(old, record.invoker, record.nonce));
        }
        self.horizons
            .insert((record.horizon, record.invoker, record.nonce));
    }

    /// Puts the whole record of a grant back, replacing any record of it:
    /// totals, expiry and rate window, exactly. Bounds are not checked.
    pub fn restore_usage(&mut self, record: &UsageRecord) {
        self.usage.insert(record.id, Usage::from_record(record));
    }

    /// The record of a grant, if the ledger has one.
    pub fn usage_record(&self, id: &GrantId) -> Option<UsageRecord> {
        self.usage.get(id).map(|usage| usage.to_record(*id))
    }

    /// Sets the totals of a grant, as if that much had already been charged.
    /// The record never expires.
    pub fn preload(&mut self, id: GrantId, calls: u64, cost: u64, bytes: u64) {
        let entry = self.usage.entry(id).or_insert_with(|| Usage::new(u64::MAX));
        entry.calls = calls;
        entry.cost = cost;
        entry.bytes = bytes;
    }

    /// Records that a call was charged to the grant at `time`.
    pub fn record_call_at(&mut self, id: GrantId, time: u64) {
        let entry = self.usage.entry(id).or_insert_with(|| Usage::new(u64::MAX));
        entry.calls = entry.calls.saturating_add(1);
        let slot = entry.window.entry(time).or_insert(0);
        *slot = slot.saturating_add(1);
    }
}

/// Whether one more charge of `decision` would put a grant with `limits` and
/// `spent` over a ceiling. The same test as step 11 of verification.
fn exceeds(spent: &Spent, limits: &Limits, decision: &Decision) -> bool {
    over(spent.calls, 1, limits.calls)
        || over(spent.cost, decision.cost, limits.cost)
        || over(spent.bytes, decision.bytes, limits.bytes)
        || over(spent.recent_calls, 1, limits.rate)
}

impl UsageLedger for MemoryLedger {
    fn nonce_seen(&self, invoker: &PublicKey, nonce: &[u8; 16]) -> bool {
        self.nonces.contains_key(&(*invoker, *nonce))
    }

    fn spent(&self, id: &GrantId, now: u64) -> Option<Spent> {
        let spent = match self.usage.get(id) {
            Some(usage) => usage.spent(now),
            None => Spent::default(),
        };
        Some(spent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grant::Effect;

    fn decision(id: u8, nonce: u8, now: u64, limits: Limits) -> Decision {
        Decision {
            chain_ids: vec![[id; 32]],
            chain_limits: vec![limits],
            chain_expiries: vec![now + 1_000],
            nonce_horizon: now + 60,
            invoker: [7u8; 32],
            nonce: [nonce; 16],
            now,
            effect: Effect::Read,
            cost: 3,
            bytes: 5,
        }
    }

    fn calls(n: u64) -> Limits {
        Limits {
            calls: Some(n),
            ..Limits::default()
        }
    }

    #[test]
    fn a_plan_changes_nothing_until_it_is_committed() {
        let mut ledger = MemoryLedger::new();
        let d = decision(1, 1, 100, calls(5));
        let plan = ledger.plan_charge(&d).unwrap();
        assert_eq!(ledger.nonce_count(), 0);
        assert_eq!(ledger.grant_count(), 0);
        assert_eq!(plan.usage.len(), 1);
        assert_eq!(plan.usage[0].calls, 1);
        assert_eq!(plan.usage[0].cost, 3);
        assert_eq!(plan.usage[0].bytes, 5);
        assert_eq!(plan.usage[0].window, vec![(100, 1)]);
        ledger.commit(plan);
        assert_eq!(ledger.nonce_count(), 1);
        assert!(ledger.nonce_seen(&[7u8; 32], &[1u8; 16]));
        assert_eq!(ledger.spent(&[1u8; 32], 100).unwrap().calls, 1);
    }

    #[test]
    fn charge_and_plan_then_commit_agree() {
        let mut direct = MemoryLedger::new();
        let mut planned = MemoryLedger::new();
        for step in 0..4u8 {
            let d = decision(1, step, 100 + u64::from(step), calls(3));
            let a = direct.charge(&d);
            let b = planned.plan_charge(&d).map(|plan| planned.commit(plan));
            assert_eq!(a, b);
        }
        assert_eq!(
            direct.usage_record(&[1u8; 32]),
            planned.usage_record(&[1u8; 32])
        );
        assert_eq!(direct.nonce_count(), planned.nonce_count());
    }

    #[test]
    fn a_refused_plan_is_an_error_and_changes_nothing() {
        let mut ledger = MemoryLedger::new();
        ledger.charge(&decision(1, 1, 100, calls(1))).unwrap();
        let before = ledger.usage_record(&[1u8; 32]);
        assert_eq!(
            ledger.plan_charge(&decision(1, 2, 101, calls(1))),
            Err(ChargeError::LimitExhausted)
        );
        assert_eq!(
            ledger.plan_charge(&decision(1, 1, 101, calls(9))),
            Err(ChargeError::NonceReplayed)
        );
        assert_eq!(ledger.usage_record(&[1u8; 32]), before);
        assert_eq!(ledger.nonce_count(), 1);
    }

    #[test]
    fn restored_records_give_the_same_answers() {
        let mut ledger = MemoryLedger::new();
        let limits = Limits {
            calls: Some(10),
            rate: Some(2),
            ..Limits::default()
        };
        ledger.charge(&decision(1, 1, 100, limits)).unwrap();
        ledger.charge(&decision(1, 2, 130, limits)).unwrap();

        let mut restored = MemoryLedger::new();
        restored.restore_usage(&ledger.usage_record(&[1u8; 32]).unwrap());
        restored.restore_nonce(NonceRecord {
            invoker: [7u8; 32],
            nonce: [1u8; 16],
            horizon: 160,
        });
        restored.restore_nonce(NonceRecord {
            invoker: [7u8; 32],
            nonce: [2u8; 16],
            horizon: 190,
        });
        for now in [130u64, 159, 160, 161, 189, 190, 191] {
            assert_eq!(
                ledger.spent(&[1u8; 32], now),
                restored.spent(&[1u8; 32], now)
            );
        }
        assert!(restored.nonce_seen(&[7u8; 32], &[1u8; 16]));
        assert_eq!(
            restored.charge(&decision(1, 1, 140, limits)),
            Err(ChargeError::NonceReplayed)
        );
        // Two calls inside the window: the rate ceiling of 2 is reached.
        assert_eq!(
            restored.charge(&decision(1, 3, 140, limits)),
            Err(ChargeError::LimitExhausted)
        );
        // The nonce horizon moves with a restored nonce that is restored twice.
        restored.restore_nonce(NonceRecord {
            invoker: [7u8; 32],
            nonce: [1u8; 16],
            horizon: 300,
        });
        assert_eq!(restored.nonce_count(), 2);
        assert_eq!(restored.expire(250).nonces, vec![([7u8; 32], [2u8; 16])]);
        assert_eq!(restored.nonce_count(), 1);
    }

    #[test]
    fn expire_reports_what_it_drops() {
        let mut ledger = MemoryLedger::new();
        ledger.charge(&decision(1, 1, 100, calls(5))).unwrap();
        let none = ledger.expire(100);
        assert!(none.nonces.is_empty() && none.grants.is_empty());
        let dropped = ledger.expire(2_000);
        assert_eq!(dropped.nonces, vec![([7u8; 32], [1u8; 16])]);
        assert_eq!(dropped.grants, vec![[1u8; 32]]);
        assert_eq!(ledger.nonce_count(), 0);
        assert_eq!(ledger.grant_count(), 0);
    }
}
