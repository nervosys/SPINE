//! A usage ledger that survives a restart (`docs/spec/03-authority.md`,
//! "Usage ledger" and "Node enforcement", decisions P37 to P43).
//!
//! [`spine_authority::MemoryLedger`] is pure and lives in memory. After a
//! restart a spent call limit would be whole again and a nonce inside its
//! replay horizon could be used a second time. [`DurableLedger`] wraps a
//! `MemoryLedger` and a [`StorageBackend`] and keeps the two in step:
//!
//! * [`DurableLedger::charge`] asks the memory ledger what the charge would
//!   leave behind ([`MemoryLedger::plan_charge`]), writes those rows to the
//!   backend, and only then applies them to memory
//!   ([`MemoryLedger::commit`]). If a write fails the charge is refused and
//!   memory is as it was: the ledger fails closed.
//! * [`DurableLedger::open`] reads the rows back, drops what is past its
//!   horizon or expiry, and rebuilds the memory ledger exactly.
//!
//! # Crash semantics
//!
//! The rows are written before the reply, so a crash between the write and
//! the reply leaves the call charged although the node never ran it. That is
//! the safe side for a limit (the grant has one call fewer than it should)
//! and for a nonce (the invoker must use a new one). The nonce row is written
//! first and the usage rows after it, in chain order. A crash or a failure in
//! between leaves a nonce that is burned with fewer grants charged than the
//! call would have charged; the command did not run, so nothing was spent
//! without being counted. A failure after some usage rows leaves those grants
//! charged for a call that was refused. The backend can therefore be ahead of
//! memory after a failed charge, never behind it.
//!
//! # Rows
//!
//! Two namespaces, each with a version in the name and in every row. All
//! integers are big-endian.
//!
//! * [`NONCE_NAMESPACE`] (`spine-authority-nonces-v1`): key invoker (32 bytes)
//!   then nonce (16 bytes); value `01` then the horizon (u64): 9 bytes.
//! * [`USAGE_NAMESPACE`] (`spine-authority-usage-v1`): key the grant id (32
//!   bytes); value `01`, calls, cost, bytes, expires (4 x u64), the number `n`
//!   of rate-window entries (u16, at most [`MAX_WINDOW_ENTRIES`]), then `n`
//!   pairs (second u64, calls u64): `35 + 16 n` bytes.
//!
//! # What is restored, and what is not
//!
//! Everything the memory ledger holds is restored exactly: nonces and their
//! horizons, and per grant the calls, cost and bytes totals, the expiry and
//! the whole rate window. A nonce is restored only if its horizon has not
//! passed, a usage row only if its grant has not expired, both relative to the
//! `now` given to [`DurableLedger::open`].
//!
//! Rows that cannot be read are never turned into "unspent":
//!
//! * a row whose key has the wrong length refuses the open
//!   ([`LedgerOpenError::BadKey`]): it cannot be tied to a nonce or a grant;
//! * a nonce row whose value does not decode is restored with a horizon that
//!   never passes;
//! * a usage row whose value does not decode is restored as exhausted: every
//!   total at `u64::MAX`, the expiry at `u64::MAX` and a rate window that
//!   never ages. Only a limit of exactly `u64::MAX` on `cost` or `bytes`, with
//!   an invocation that declares none, is not refused by it.
//!
//! Both are counted in [`OpenReport`]; an operator removes such a row to
//! release the grant or the nonce.
//!
//! # Bounds
//!
//! The same [`LedgerConfig`] as the memory ledger. A charge that would pass a
//! bound is refused before anything is written. If the rows found on open
//! already exceed a bound, all are restored and charges are refused
//! (`LedgerFull`) until enough of them expire. Deleting the rows of expired
//! nonces and grants is done at most [`PRUNE_BATCH`] rows per charge, and by
//! [`DurableLedger::prune`]; a failed delete is retried later and never
//! refuses a charge.
//!
//! # Not done
//!
//! The backend has no multi-key transaction here, so the write is not atomic
//! across rows (see the crash semantics). Two nodes sharing one backend are
//! not supported: the ledger assumes it is the only writer. The backend's own
//! durability (fsync) is whatever the chosen [`StorageBackend`] gives.

use std::collections::BTreeSet;
use std::sync::Arc;

use spine_authority::{
    ChargeError, Decision, GrantId, LedgerConfig, MemoryLedger, NonceRecord, PublicKey, Spent,
    UsageLedger, UsageRecord,
};
use spine_storage::StorageBackend;

/// Namespace of the nonce rows.
pub const NONCE_NAMESPACE: &str = "spine-authority-nonces-v1";

/// Namespace of the usage rows.
pub const USAGE_NAMESPACE: &str = "spine-authority-usage-v1";

/// Most rate-window entries a usage row may hold. A window holds one entry per
/// second of the last minute under a clock that does not run backwards.
pub const MAX_WINDOW_ENTRIES: usize = 1024;

/// Most rows deleted by one [`DurableLedger::charge`].
pub const PRUNE_BATCH: usize = 64;

const ROW_VERSION: u8 = 1;
const NONCE_KEY_LEN: usize = 48;
const NONCE_VALUE_LEN: usize = 9;
const USAGE_HEADER_LEN: usize = 35;
const USAGE_ENTRY_LEN: usize = 16;

type NonceKey = (PublicKey, [u8; 16]);

/// Why [`DurableLedger::open`] refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerOpenError {
    /// The backend could not be read.
    Storage(String),
    /// A row has a key of the wrong length: it cannot be tied to a nonce or a
    /// grant, so the ledger cannot say what it would have refused.
    BadKey {
        /// The namespace of the row.
        namespace: &'static str,
        /// The length of its key.
        len: usize,
    },
}

impl std::fmt::Display for LedgerOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LedgerOpenError::Storage(message) => {
                write!(f, "the authority ledger could not be read: {message}")
            }
            LedgerOpenError::BadKey { namespace, len } => write!(
                f,
                "the authority ledger has a row with a {len}-byte key in {namespace}"
            ),
        }
    }
}

impl std::error::Error for LedgerOpenError {}

/// Why [`DurableLedger::charge`] charged nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableChargeError {
    /// The memory ledger refused.
    Charge(ChargeError),
    /// The rows could not be written, so the charge was refused.
    Storage,
}

impl std::fmt::Display for DurableChargeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DurableChargeError::Charge(error) => write!(f, "{error}"),
            DurableChargeError::Storage => f.write_str("the usage ledger could not be written"),
        }
    }
}

impl std::error::Error for DurableChargeError {}

/// What [`DurableLedger::open`] found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpenReport {
    /// Nonce rows restored.
    pub nonces_restored: usize,
    /// Nonce rows past their horizon, not restored and queued for deletion.
    pub nonces_expired: usize,
    /// Nonce rows whose value did not decode, restored with a horizon that
    /// never passes.
    pub nonces_unreadable: usize,
    /// Usage rows restored.
    pub usage_restored: usize,
    /// Usage rows of expired grants, not restored and queued for deletion.
    pub usage_expired: usize,
    /// Usage rows whose value did not decode, restored as exhausted.
    pub usage_unreadable: usize,
}

fn encode_nonce(horizon: u64) -> [u8; NONCE_VALUE_LEN] {
    let mut value = [0u8; NONCE_VALUE_LEN];
    value[0] = ROW_VERSION;
    value[1..].copy_from_slice(&horizon.to_be_bytes());
    value
}

fn decode_nonce(value: &[u8]) -> Option<u64> {
    if value.len() != NONCE_VALUE_LEN || value[0] != ROW_VERSION {
        return None;
    }
    read_u64(value, 1)
}

fn nonce_key(invoker: &PublicKey, nonce: &[u8; 16]) -> [u8; NONCE_KEY_LEN] {
    let mut key = [0u8; NONCE_KEY_LEN];
    key[..32].copy_from_slice(invoker);
    key[32..].copy_from_slice(nonce);
    key
}

fn parse_nonce_key(key: &[u8]) -> Option<NonceKey> {
    if key.len() != NONCE_KEY_LEN {
        return None;
    }
    let mut invoker = [0u8; 32];
    let mut nonce = [0u8; 16];
    invoker.copy_from_slice(&key[..32]);
    nonce.copy_from_slice(&key[32..]);
    Some((invoker, nonce))
}

fn parse_grant_key(key: &[u8]) -> Option<GrantId> {
    GrantId::try_from(key).ok()
}

fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    let part = bytes.get(at..at.checked_add(8)?)?;
    Some(u64::from_be_bytes(part.try_into().ok()?))
}

/// The value of a usage row, or `None` if its window is over
/// [`MAX_WINDOW_ENTRIES`].
fn encode_usage(record: &UsageRecord) -> Option<Vec<u8>> {
    if record.window.len() > MAX_WINDOW_ENTRIES {
        return None;
    }
    let count = u16::try_from(record.window.len()).ok()?;
    let mut value = Vec::with_capacity(USAGE_HEADER_LEN + USAGE_ENTRY_LEN * record.window.len());
    value.push(ROW_VERSION);
    value.extend_from_slice(&record.calls.to_be_bytes());
    value.extend_from_slice(&record.cost.to_be_bytes());
    value.extend_from_slice(&record.bytes.to_be_bytes());
    value.extend_from_slice(&record.expires.to_be_bytes());
    value.extend_from_slice(&count.to_be_bytes());
    for (second, calls) in &record.window {
        value.extend_from_slice(&second.to_be_bytes());
        value.extend_from_slice(&calls.to_be_bytes());
    }
    Some(value)
}

fn decode_usage(id: GrantId, value: &[u8]) -> Option<UsageRecord> {
    if value.len() < USAGE_HEADER_LEN || value[0] != ROW_VERSION {
        return None;
    }
    let count = usize::from(u16::from_be_bytes(value.get(33..35)?.try_into().ok()?));
    if count > MAX_WINDOW_ENTRIES || value.len() != USAGE_HEADER_LEN + USAGE_ENTRY_LEN * count {
        return None;
    }
    let mut window = Vec::with_capacity(count);
    for index in 0..count {
        let at = USAGE_HEADER_LEN + USAGE_ENTRY_LEN * index;
        window.push((read_u64(value, at)?, read_u64(value, at + 8)?));
    }
    Some(UsageRecord {
        id,
        calls: read_u64(value, 1)?,
        cost: read_u64(value, 9)?,
        bytes: read_u64(value, 17)?,
        expires: read_u64(value, 25)?,
        window,
    })
}

/// The record that refuses every limit, for a row that cannot be read.
fn exhausted(id: GrantId) -> UsageRecord {
    UsageRecord {
        id,
        calls: u64::MAX,
        cost: u64::MAX,
        bytes: u64::MAX,
        expires: u64::MAX,
        window: vec![(u64::MAX, u64::MAX)],
    }
}

/// A [`MemoryLedger`] mirrored in a [`StorageBackend`]. See the module
/// documentation.
pub struct DurableLedger {
    memory: MemoryLedger,
    backend: Arc<dyn StorageBackend>,
    /// Rows of expired nonces still in the backend.
    stale_nonces: BTreeSet<NonceKey>,
    /// Rows of expired grants still in the backend.
    stale_grants: BTreeSet<GrantId>,
    report: OpenReport,
}

impl std::fmt::Debug for DurableLedger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DurableLedger")
            .field("memory", &self.memory)
            .field("pending_deletes", &self.pending_deletes())
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

impl DurableLedger {
    /// Opens the ledger kept in `backend`, with the bounds `config`, as of
    /// time `now` (Unix seconds, the node's clock). Rows past their horizon or
    /// expiry are not restored; see the module documentation for rows that
    /// cannot be read.
    pub fn open(
        backend: Arc<dyn StorageBackend>,
        config: LedgerConfig,
        now: u64,
    ) -> Result<DurableLedger, LedgerOpenError> {
        let mut memory = MemoryLedger::with_config(config);
        let mut stale_nonces = BTreeSet::new();
        let mut stale_grants = BTreeSet::new();
        let mut report = OpenReport::default();

        let rows = backend
            .scan(NONCE_NAMESPACE, None)
            .map_err(|error| LedgerOpenError::Storage(error.to_string()))?;
        for (key, value) in rows {
            let Some((invoker, nonce)) = parse_nonce_key(&key) else {
                return Err(LedgerOpenError::BadKey {
                    namespace: NONCE_NAMESPACE,
                    len: key.len(),
                });
            };
            let horizon = match decode_nonce(&value) {
                Some(horizon) => horizon,
                None => {
                    report.nonces_unreadable += 1;
                    u64::MAX
                }
            };
            if horizon < now {
                stale_nonces.insert((invoker, nonce));
                report.nonces_expired += 1;
            } else {
                memory.restore_nonce(NonceRecord {
                    invoker,
                    nonce,
                    horizon,
                });
                report.nonces_restored += 1;
            }
        }

        let rows = backend
            .scan(USAGE_NAMESPACE, None)
            .map_err(|error| LedgerOpenError::Storage(error.to_string()))?;
        for (key, value) in rows {
            let Some(id) = parse_grant_key(&key) else {
                return Err(LedgerOpenError::BadKey {
                    namespace: USAGE_NAMESPACE,
                    len: key.len(),
                });
            };
            let record = match decode_usage(id, &value) {
                Some(record) => record,
                None => {
                    report.usage_unreadable += 1;
                    exhausted(id)
                }
            };
            if record.expires <= now {
                stale_grants.insert(id);
                report.usage_expired += 1;
            } else {
                memory.restore_usage(&record);
                report.usage_restored += 1;
            }
        }

        Ok(DurableLedger {
            memory,
            backend,
            stale_nonces,
            stale_grants,
            report,
        })
    }

    /// What [`DurableLedger::open`] found.
    pub fn open_report(&self) -> OpenReport {
        self.report
    }

    /// Number of nonces remembered.
    pub fn nonce_count(&self) -> usize {
        self.memory.nonce_count()
    }

    /// Number of grants with a usage record.
    pub fn grant_count(&self) -> usize {
        self.memory.grant_count()
    }

    /// Rows of expired nonces and grants not yet deleted from the backend.
    pub fn pending_deletes(&self) -> usize {
        self.stale_nonces.len() + self.stale_grants.len()
    }

    /// Records an accepted invocation, as [`MemoryLedger::charge`] does, and
    /// writes it to the backend first. On any error nothing was recorded in
    /// memory.
    pub fn charge(&mut self, decision: &Decision) -> Result<(), DurableChargeError> {
        let expired = self.memory.expire(decision.now);
        self.stale_nonces.extend(expired.nonces);
        self.stale_grants.extend(expired.grants);

        let plan = self
            .memory
            .plan_charge(decision)
            .map_err(DurableChargeError::Charge)?;

        // Encode everything before the first write.
        let mut usage_rows = Vec::with_capacity(plan.usage.len());
        for record in &plan.usage {
            match encode_usage(record) {
                Some(value) => usage_rows.push((record.id, value)),
                None => {
                    log::error!("authority ledger: a rate window is too large to store");
                    return Err(DurableChargeError::Storage);
                }
            }
        }

        let key: NonceKey = (plan.nonce.invoker, plan.nonce.nonce);
        self.stale_nonces.remove(&key);
        let written = self.backend.put(
            NONCE_NAMESPACE,
            &nonce_key(&key.0, &key.1),
            &encode_nonce(plan.nonce.horizon),
        );
        if let Err(error) = written {
            log::error!("authority ledger: writing a nonce failed: {error}");
            return Err(DurableChargeError::Storage);
        }
        for (id, value) in &usage_rows {
            self.stale_grants.remove(id);
            if let Err(error) = self.backend.put(USAGE_NAMESPACE, id, value) {
                log::error!("authority ledger: writing a usage row failed: {error}");
                return Err(DurableChargeError::Storage);
            }
        }

        self.memory.commit(plan);
        self.prune(PRUNE_BATCH);
        Ok(())
    }

    /// Deletes up to `budget` rows of expired nonces and grants from the
    /// backend and returns how many it deleted. A delete that fails is left
    /// for a later call.
    pub fn prune(&mut self, budget: usize) -> usize {
        let mut deleted = 0usize;
        while deleted < budget {
            let Some(key) = self.stale_nonces.iter().next().copied() else {
                break;
            };
            match self
                .backend
                .delete(NONCE_NAMESPACE, &nonce_key(&key.0, &key.1))
            {
                Ok(()) => {
                    self.stale_nonces.remove(&key);
                    deleted += 1;
                }
                Err(error) => {
                    log::warn!("authority ledger: deleting an expired nonce failed: {error}");
                    return deleted;
                }
            }
        }
        while deleted < budget {
            let Some(id) = self.stale_grants.iter().next().copied() else {
                break;
            };
            match self.backend.delete(USAGE_NAMESPACE, &id) {
                Ok(()) => {
                    self.stale_grants.remove(&id);
                    deleted += 1;
                }
                Err(error) => {
                    log::warn!("authority ledger: deleting an expired usage row failed: {error}");
                    return deleted;
                }
            }
        }
        deleted
    }
}

impl UsageLedger for DurableLedger {
    fn nonce_seen(&self, invoker: &PublicKey, nonce: &[u8; 16]) -> bool {
        self.memory.nonce_seen(invoker, nonce)
    }

    fn spent(&self, id: &GrantId, now: u64) -> Option<Spent> {
        self.memory.spent(id, now)
    }
}

/// A backend that can be told to fail, for tests.
#[cfg(test)]
pub(crate) struct FlakyBackend {
    inner: spine_storage::InMemoryBackend,
    puts_allowed: std::sync::atomic::AtomicUsize,
    deletes_fail: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl FlakyBackend {
    pub(crate) fn new() -> FlakyBackend {
        FlakyBackend {
            inner: spine_storage::InMemoryBackend::new(),
            puts_allowed: std::sync::atomic::AtomicUsize::new(usize::MAX),
            deletes_fail: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// The next `n` puts succeed and every one after them fails.
    pub(crate) fn allow_puts(&self, n: usize) {
        self.puts_allowed
            .store(n, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn fail_deletes(&self, on: bool) {
        self.deletes_fail
            .store(on, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
impl StorageBackend for FlakyBackend {
    fn get(&self, namespace: &str, key: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        self.inner.get(namespace, key)
    }

    fn put(&self, namespace: &str, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        use std::sync::atomic::Ordering;
        let left = self.puts_allowed.load(Ordering::SeqCst);
        if left == 0 {
            return Err(anyhow::anyhow!("injected put failure"));
        }
        if left != usize::MAX {
            self.puts_allowed.store(left - 1, Ordering::SeqCst);
        }
        self.inner.put(namespace, key, value)
    }

    fn delete(&self, namespace: &str, key: &[u8]) -> anyhow::Result<()> {
        if self.deletes_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(anyhow::anyhow!("injected delete failure"));
        }
        self.inner.delete(namespace, key)
    }

    fn scan(
        &self,
        namespace: &str,
        prefix: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, Vec<u8>)>> {
        self.inner.scan(namespace, prefix)
    }

    fn keys(&self, namespace: &str) -> anyhow::Result<Vec<Vec<u8>>> {
        self.inner.keys(namespace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spine_authority::{Effect, Limits};

    const ID: GrantId = [1u8; 32];
    const INVOKER: PublicKey = [7u8; 32];

    fn decision(id: u8, nonce: u8, now: u64, limits: Limits) -> Decision {
        Decision {
            chain_ids: vec![[id; 32]],
            chain_limits: vec![limits],
            chain_expiries: vec![now + 1_000],
            nonce_horizon: now + 60,
            invoker: INVOKER,
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

    fn open_on(backend: &Arc<FlakyBackend>, now: u64) -> DurableLedger {
        let backend: Arc<dyn StorageBackend> = backend.clone();
        DurableLedger::open(backend, LedgerConfig::default(), now).unwrap()
    }

    fn rows(backend: &Arc<FlakyBackend>) -> (usize, usize) {
        (
            backend.count(NONCE_NAMESPACE).unwrap(),
            backend.count(USAGE_NAMESPACE).unwrap(),
        )
    }

    #[test]
    fn row_codecs_round_trip_and_refuse_malformed_values() {
        assert_eq!(decode_nonce(&encode_nonce(77)), Some(77));
        assert_eq!(decode_nonce(&[1u8; 8]), None);
        let mut bad = encode_nonce(77);
        bad[0] = 2;
        assert_eq!(decode_nonce(&bad), None);

        let record = UsageRecord {
            id: ID,
            calls: 3,
            cost: 4,
            bytes: 5,
            expires: 600,
            window: vec![(10, 1), (11, 2)],
        };
        let value = encode_usage(&record).unwrap();
        assert_eq!(value.len(), USAGE_HEADER_LEN + 2 * USAGE_ENTRY_LEN);
        assert_eq!(decode_usage(ID, &value), Some(record.clone()));
        assert_eq!(decode_usage(ID, &value[..value.len() - 1]), None);
        let mut longer = value.clone();
        longer.push(0);
        assert_eq!(decode_usage(ID, &longer), None);
        let mut wrong_version = value.clone();
        wrong_version[0] = 9;
        assert_eq!(decode_usage(ID, &wrong_version), None);
        let mut wrong_count = value;
        wrong_count[34] = 3;
        assert_eq!(decode_usage(ID, &wrong_count), None);

        let too_wide = UsageRecord {
            window: (0..=MAX_WINDOW_ENTRIES as u64).map(|s| (s, 1)).collect(),
            ..record
        };
        assert_eq!(encode_usage(&too_wide), None);
    }

    #[test]
    fn a_charged_call_limit_survives_reopen() {
        let backend = Arc::new(FlakyBackend::new());
        let mut ledger = open_on(&backend, 100);
        ledger.charge(&decision(1, 1, 100, calls(2))).unwrap();
        ledger.charge(&decision(1, 2, 101, calls(2))).unwrap();
        assert_eq!(
            ledger.charge(&decision(1, 3, 102, calls(2))),
            Err(DurableChargeError::Charge(ChargeError::LimitExhausted))
        );
        drop(ledger);

        let mut reopened = open_on(&backend, 110);
        assert_eq!(reopened.spent(&ID, 110).unwrap().calls, 2);
        assert_eq!(
            reopened.charge(&decision(1, 3, 110, calls(2))),
            Err(DurableChargeError::Charge(ChargeError::LimitExhausted))
        );
        assert_eq!(
            reopened.open_report(),
            OpenReport {
                nonces_restored: 2,
                usage_restored: 1,
                ..OpenReport::default()
            }
        );
    }

    #[test]
    fn totals_and_rate_window_are_restored_exactly() {
        let backend = Arc::new(FlakyBackend::new());
        let limits = Limits {
            calls: Some(50),
            cost: Some(100),
            bytes: Some(100),
            rate: Some(2),
        };
        let mut ledger = open_on(&backend, 100);
        ledger.charge(&decision(1, 1, 100, limits)).unwrap();
        ledger.charge(&decision(1, 2, 130, limits)).unwrap();
        let reopened = open_on(&backend, 135);
        for now in [130u64, 150, 159, 160, 161, 189, 190, 191] {
            assert_eq!(ledger.spent(&ID, now), reopened.spent(&ID, now), "{now}");
        }
        assert_eq!(reopened.spent(&ID, 135).unwrap().cost, 6);
        assert_eq!(reopened.spent(&ID, 135).unwrap().bytes, 10);
        assert_eq!(reopened.spent(&ID, 135).unwrap().recent_calls, 2);
    }

    #[test]
    fn a_nonce_inside_its_horizon_is_refused_after_reopen() {
        let backend = Arc::new(FlakyBackend::new());
        let mut ledger = open_on(&backend, 100);
        ledger
            .charge(&decision(1, 1, 100, Limits::default()))
            .unwrap();
        drop(ledger);

        let mut reopened = open_on(&backend, 150);
        assert!(reopened.nonce_seen(&INVOKER, &[1u8; 16]));
        assert_eq!(
            reopened.charge(&decision(1, 1, 150, Limits::default())),
            Err(DurableChargeError::Charge(ChargeError::NonceReplayed))
        );
        // A grant without a limit is never tracked.
        assert_eq!(reopened.grant_count(), 0);
        assert_eq!(rows(&backend), (1, 0));
    }

    #[test]
    fn expired_rows_are_dropped_on_open_and_deleted_in_bounded_batches() {
        let backend = Arc::new(FlakyBackend::new());
        let mut ledger = open_on(&backend, 100);
        for nonce in 0..10u8 {
            ledger
                .charge(&decision(nonce, nonce, 100, calls(5)))
                .unwrap();
        }
        assert_eq!(rows(&backend), (10, 10));
        drop(ledger);

        // Horizon 160, grant expiry 1100.
        let mut reopened = open_on(&backend, 161);
        assert_eq!(reopened.nonce_count(), 0);
        assert_eq!(reopened.grant_count(), 10);
        assert_eq!(reopened.open_report().nonces_expired, 10);
        assert_eq!(reopened.pending_deletes(), 10);
        assert!(!reopened.nonce_seen(&INVOKER, &[1u8; 16]));
        assert_eq!(reopened.prune(4), 4);
        assert_eq!(reopened.pending_deletes(), 6);
        assert_eq!(reopened.prune(100), 6);
        assert_eq!(rows(&backend), (0, 10));

        let late = open_on(&backend, 1_100);
        assert_eq!(late.grant_count(), 0);
        assert_eq!(late.open_report().usage_expired, 10);
        assert_eq!(late.pending_deletes(), 10);
    }

    #[test]
    fn a_charge_prunes_what_has_expired_and_a_failed_delete_does_not_refuse_it() {
        let backend = Arc::new(FlakyBackend::new());
        let mut ledger = open_on(&backend, 100);
        ledger.charge(&decision(1, 1, 100, calls(9))).unwrap();
        backend.fail_deletes(true);
        ledger.charge(&decision(1, 2, 200, calls(9))).unwrap();
        assert_eq!(ledger.pending_deletes(), 1);
        assert_eq!(rows(&backend).0, 2);
        backend.fail_deletes(false);
        ledger.charge(&decision(1, 3, 201, calls(9))).unwrap();
        assert_eq!(ledger.pending_deletes(), 0);
        // Nonce 1 is gone; 2 and 3 remain.
        assert_eq!(rows(&backend).0, 2);
    }

    #[test]
    fn a_nonce_used_again_after_its_horizon_is_not_deleted_by_the_old_delete() {
        let backend = Arc::new(FlakyBackend::new());
        let mut ledger = open_on(&backend, 100);
        ledger
            .charge(&decision(1, 1, 100, Limits::default()))
            .unwrap();
        drop(ledger);
        let mut reopened = open_on(&backend, 200);
        assert_eq!(reopened.pending_deletes(), 1);
        // The same invoker and nonce, past the old horizon: accepted again.
        reopened
            .charge(&decision(1, 1, 200, Limits::default()))
            .unwrap();
        assert_eq!(reopened.pending_deletes(), 0);
        reopened.prune(100);
        assert_eq!(rows(&backend).0, 1);
        let again = open_on(&backend, 210);
        assert!(again.nonce_seen(&INVOKER, &[1u8; 16]));
    }

    #[test]
    fn a_failed_write_refuses_the_charge_and_leaves_memory_unchanged() {
        let backend = Arc::new(FlakyBackend::new());
        let mut ledger = open_on(&backend, 100);
        backend.allow_puts(0);
        assert_eq!(
            ledger.charge(&decision(1, 1, 100, calls(1))),
            Err(DurableChargeError::Storage)
        );
        assert_eq!(ledger.nonce_count(), 0);
        assert_eq!(ledger.grant_count(), 0);
        assert_eq!(ledger.spent(&ID, 100).unwrap().calls, 0);
        assert!(!ledger.nonce_seen(&INVOKER, &[1u8; 16]));
        assert_eq!(rows(&backend), (0, 0));

        // The backend recovers: the same invocation can be charged once.
        backend.allow_puts(usize::MAX);
        ledger.charge(&decision(1, 1, 100, calls(1))).unwrap();
        assert_eq!(ledger.spent(&ID, 100).unwrap().calls, 1);
        assert_eq!(rows(&backend), (1, 1));
    }

    #[test]
    fn a_failure_between_the_rows_errs_on_the_side_of_having_charged() {
        let backend = Arc::new(FlakyBackend::new());
        let mut ledger = open_on(&backend, 100);
        // The nonce row is written, the usage row is not.
        backend.allow_puts(1);
        assert_eq!(
            ledger.charge(&decision(1, 1, 100, calls(1))),
            Err(DurableChargeError::Storage)
        );
        assert_eq!(ledger.nonce_count(), 0);
        assert_eq!(ledger.spent(&ID, 100).unwrap().calls, 0);
        assert_eq!(rows(&backend), (1, 0));
        drop(ledger);

        // After a restart the nonce is burned; nothing was charged for it.
        let mut reopened = open_on(&backend, 101);
        assert!(reopened.nonce_seen(&INVOKER, &[1u8; 16]));
        assert_eq!(reopened.spent(&ID, 101).unwrap().calls, 0);
        reopened.charge(&decision(1, 2, 101, calls(1))).unwrap();
    }

    #[test]
    fn an_unreadable_usage_row_restores_the_grant_as_exhausted() {
        let backend = Arc::new(FlakyBackend::new());
        backend
            .put(USAGE_NAMESPACE, &[1u8; 32], b"garbage")
            .unwrap();
        backend.put(USAGE_NAMESPACE, &[2u8; 32], &[]).unwrap();
        let mut ledger = open_on(&backend, 100);
        assert_eq!(ledger.open_report().usage_unreadable, 2);
        assert_eq!(ledger.grant_count(), 2);
        for limits in [
            calls(1_000),
            Limits {
                rate: Some(1_000),
                ..Limits::default()
            },
            Limits {
                cost: Some(1_000),
                ..Limits::default()
            },
        ] {
            assert_eq!(
                ledger.charge(&decision(1, 1, 5_000, limits)),
                Err(DurableChargeError::Charge(ChargeError::LimitExhausted))
            );
        }
        // Other grants are not affected.
        ledger.charge(&decision(3, 1, 100, calls(1))).unwrap();
    }

    #[test]
    fn an_unreadable_nonce_row_is_remembered_and_a_bad_key_refuses_to_open() {
        let backend = Arc::new(FlakyBackend::new());
        backend
            .put(NONCE_NAMESPACE, &nonce_key(&INVOKER, &[4u8; 16]), b"x")
            .unwrap();
        let mut ledger = open_on(&backend, 100);
        assert_eq!(ledger.open_report().nonces_unreadable, 1);
        assert!(ledger.nonce_seen(&INVOKER, &[4u8; 16]));
        assert_eq!(
            ledger.charge(&decision(1, 4, 1_000_000, Limits::default())),
            Err(DurableChargeError::Charge(ChargeError::NonceReplayed))
        );

        for (namespace, key) in [
            (NONCE_NAMESPACE, vec![0u8; 47]),
            (USAGE_NAMESPACE, vec![0u8; 31]),
        ] {
            let backend = Arc::new(FlakyBackend::new());
            backend.put(namespace, &key, &[]).unwrap();
            let as_dyn: Arc<dyn StorageBackend> = backend;
            let error = DurableLedger::open(as_dyn, LedgerConfig::default(), 0).unwrap_err();
            assert_eq!(
                error,
                LedgerOpenError::BadKey {
                    namespace,
                    len: key.len()
                }
            );
        }
    }

    #[test]
    fn the_bounds_hold_and_a_refused_charge_writes_nothing() {
        let backend = Arc::new(FlakyBackend::new());
        let as_dyn: Arc<dyn StorageBackend> = backend.clone();
        let config = LedgerConfig {
            max_nonces: 2,
            max_grants: 1,
        };
        let mut ledger = DurableLedger::open(as_dyn, config, 100).unwrap();
        ledger.charge(&decision(1, 1, 100, calls(5))).unwrap();
        assert_eq!(
            ledger.charge(&decision(2, 2, 100, calls(5))),
            Err(DurableChargeError::Charge(ChargeError::LedgerFull))
        );
        ledger.charge(&decision(1, 2, 100, calls(5))).unwrap();
        assert_eq!(
            ledger.charge(&decision(1, 3, 100, calls(5))),
            Err(DurableChargeError::Charge(ChargeError::LedgerFull))
        );
        assert_eq!(rows(&backend), (2, 1));

        // After the horizon the nonces go and room is made, in memory and in
        // the backend.
        ledger.charge(&decision(1, 3, 200, calls(5))).unwrap();
        assert_eq!(ledger.nonce_count(), 1);
        assert_eq!(rows(&backend), (1, 1));
        drop(ledger);

        // Rows over a smaller bound are all restored and refuse charges.
        let smaller = LedgerConfig {
            max_nonces: 0,
            max_grants: 0,
        };
        let as_dyn: Arc<dyn StorageBackend> = backend.clone();
        let mut over = DurableLedger::open(as_dyn, smaller, 200).unwrap();
        assert_eq!(over.nonce_count(), 1);
        assert_eq!(over.grant_count(), 1);
        assert_eq!(
            over.charge(&decision(1, 9, 201, calls(5))),
            Err(DurableChargeError::Charge(ChargeError::LedgerFull))
        );
    }

    #[test]
    fn a_malformed_decision_is_refused_before_any_write() {
        let backend = Arc::new(FlakyBackend::new());
        let mut ledger = open_on(&backend, 100);
        let mut bad = decision(1, 1, 100, calls(5));
        bad.chain_expiries.clear();
        assert_eq!(
            ledger.charge(&bad),
            Err(DurableChargeError::Charge(ChargeError::MalformedDecision))
        );
        assert_eq!(rows(&backend), (0, 0));
    }
}
