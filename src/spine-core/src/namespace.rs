//! Joining the `spine://` namespace as a mesh node.
//!
//! [`crate::config::NamespaceConfig`] says *whether and where* to join;
//! everything here is the act of joining. The node ends up with three things it
//! did not have before: a stable position in the DHT keyspace, a listener other
//! nodes can reach it on, and at least one contact to route through.
//!
//! ## Why the key is on disk
//!
//! A SPINE node's Ed25519 key is not only a credential — it *is* its keyspace
//! position, and the authority half of every `did:` name it publishes. A node
//! that generated a fresh key each start would land somewhere new in the DHT
//! every time, abandon every name it had published, and force its peers to keep
//! routing entries for a node that no longer exists. So the key is persisted,
//! and the file is the node's identity in a stronger sense than a config value.
//!
//! ## Why joining is not the end of it
//!
//! A node that joins and then sits still slowly stops being useful. Records are
//! stored at the nodes closest to their keys *as of the moment they were
//! published*, and that set changes as peers come and go — so [`join_with_application`] also
//! starts a maintenance task that re-offers held records to whoever is closest
//! now, and drops the ones that have expired.
//!
//! ## Why joining is allowed to fail
//!
//! [`join_with_application`] returns the node even when no seed answered. A namespace this node
//! cannot reach is a degraded state, not a broken one: it still serves the names
//! it holds locally, and refusing to start would take a working origin offline
//! because a seed was down. The failure is reported, loudly, and the node runs.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use spine_mesh::identity::SigningIdentity;
use spine_mesh::logbook_store::{LogBookRows, LogBookStore, LogStoreError};
use spine_mesh::mesh::{MeshConfig, MeshNode};
use spine_mesh::mesh_tcp::{serve_node_with, EncryptionConfig, Security, TcpNameTransport};
use spine_mesh::naming_mesh::{BootstrapReport, MaintenancePolicy, MeshNameResolver};
use tracing::{info, warn};
use zeroize::Zeroizing;

use crate::config::NamespaceConfig;

/// Storage namespace of the mesh service's succession logs: key is the 32-byte
/// inception key, value the log as serde JSON. Distinct from the local
/// resolver's `spine-name-successions-v1` (spec 02, 10.10 decision 50).
pub const MESH_SUCCESSIONS_NAMESPACE: &str = "spine-mesh-successions-v1";

/// Storage namespace of the mesh service's equivocation proofs: key is the
/// 32-byte inception key, value the proof as serde JSON.
pub const MESH_EQUIVOCATIONS_NAMESPACE: &str = "spine-mesh-equivocations-v1";

/// The mesh name service's log book on a [`spine_storage::StorageBackend`]:
/// logs and equivocation proofs only, never the replica records. The rows are
/// the local resolver's (a 32-byte key and the serde JSON of the log or proof)
/// in namespaces of their own, so the two books never share a row even on one
/// backend file. Rules (verify at load, bounds, write before apply) are the
/// service's: this type only reads and writes rows.
pub struct BackendLogBookStore {
    backend: Arc<dyn spine_storage::StorageBackend>,
}

impl BackendLogBookStore {
    pub fn new(backend: Arc<dyn spine_storage::StorageBackend>) -> Self {
        Self { backend }
    }

    /// Delete a row `load` could not decode, so the next start does not count it
    /// again. A failure is logged and counted in `rows`, never fatal.
    fn delete_unreadable(&self, namespace: &str, key: &[u8], rows: &mut LogBookRows) {
        if let Err(error) = self.backend.delete(namespace, key) {
            rows.delete_failures += 1;
            warn!(%error, namespace, "succession log book: an unreadable row could not be deleted");
        }
    }
}

impl std::fmt::Debug for BackendLogBookStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendLogBookStore")
            .finish_non_exhaustive()
    }
}

fn store_error(e: impl std::fmt::Display) -> LogStoreError {
    LogStoreError(e.to_string())
}

impl LogBookStore for BackendLogBookStore {
    fn load(&self) -> Result<LogBookRows, LogStoreError> {
        let mut rows = LogBookRows::default();
        for (key, data) in self
            .backend
            .scan(MESH_EQUIVOCATIONS_NAMESPACE, None)
            .map_err(store_error)?
        {
            match (
                <[u8; 32]>::try_from(key.as_slice()),
                serde_json::from_slice::<spine_name::Equivocation>(&data),
            ) {
                (Ok(key), Ok(proof)) => rows.proofs.push((key, proof)),
                _ => {
                    rows.unreadable += 1;
                    self.delete_unreadable(MESH_EQUIVOCATIONS_NAMESPACE, &key, &mut rows);
                }
            }
        }
        for (key, data) in self
            .backend
            .scan(MESH_SUCCESSIONS_NAMESPACE, None)
            .map_err(store_error)?
        {
            match (
                <[u8; 32]>::try_from(key.as_slice()),
                serde_json::from_slice::<spine_name::SuccessionLog>(&data),
            ) {
                (Ok(key), Ok(log)) => rows.logs.push((key, log)),
                _ => {
                    rows.unreadable += 1;
                    self.delete_unreadable(MESH_SUCCESSIONS_NAMESPACE, &key, &mut rows);
                }
            }
        }
        Ok(rows)
    }

    fn save_log(
        &self,
        inception_key: &[u8; 32],
        log: &spine_name::SuccessionLog,
    ) -> Result<(), LogStoreError> {
        let bytes = serde_json::to_vec(log).map_err(store_error)?;
        self.backend
            .put(MESH_SUCCESSIONS_NAMESPACE, inception_key, &bytes)
            .map_err(store_error)
    }

    fn save_proof(
        &self,
        inception_key: &[u8; 32],
        proof: &spine_name::Equivocation,
    ) -> Result<(), LogStoreError> {
        let bytes = serde_json::to_vec(proof).map_err(store_error)?;
        self.backend
            .put(MESH_EQUIVOCATIONS_NAMESPACE, inception_key, &bytes)
            .map_err(store_error)
    }

    fn remove_log(&self, inception_key: &[u8; 32]) -> Result<(), LogStoreError> {
        self.backend
            .delete(MESH_SUCCESSIONS_NAMESPACE, inception_key)
            .map_err(store_error)
    }

    fn remove_proof(&self, inception_key: &[u8; 32]) -> Result<(), LogStoreError> {
        self.backend
            .delete(MESH_EQUIVOCATIONS_NAMESPACE, inception_key)
            .map_err(store_error)
    }
}

/// What the namespace tasks did, for `/metrics` and `/ready`: the outcomes the
/// logs already reported, as counters. Created once where the daemon state is
/// built and shared by the join and the owner renewal task; they only count
/// from process start.
#[derive(Debug, Default)]
pub struct NamespaceCounters {
    /// Records re-signed and persisted by owner renewal.
    pub renewals_renewed: AtomicU64,
    /// Renewal passes that could not plan, and renewed records that could not
    /// be persisted.
    pub renewals_failed: AtomicU64,
    /// Renewed records saved locally whose publication to the mesh failed
    /// (also counted in `renewals_renewed`).
    pub renewals_publish_failed: AtomicU64,
    /// Rotated names renewal declined to sign because the node key is only the
    /// name's superseded inception key, once per name per pass.
    pub renewals_skipped_rotated: AtomicU64,
    /// Seeds that answered the bootstrap at join.
    pub seeds_reached: AtomicU64,
    /// Seeds that did not answer the bootstrap at join.
    pub seeds_failed: AtomicU64,
}

impl NamespaceCounters {
    pub fn snapshot(&self) -> spine_core::operations::NamespaceCounts {
        spine_core::operations::NamespaceCounts {
            renewals_renewed: self.renewals_renewed.load(Ordering::Relaxed),
            renewals_failed: self.renewals_failed.load(Ordering::Relaxed),
            renewals_publish_failed: self.renewals_publish_failed.load(Ordering::Relaxed),
            renewals_skipped_rotated: self.renewals_skipped_rotated.load(Ordering::Relaxed),
            seeds_reached: self.seeds_reached.load(Ordering::Relaxed),
            seeds_failed: self.seeds_failed.load(Ordering::Relaxed),
        }
    }

    /// Record what one bootstrap achieved.
    fn record_bootstrap(&self, report: &BootstrapReport) {
        self.seeds_reached
            .fetch_add(report.reached.len() as u64, Ordering::Relaxed);
        self.seeds_failed
            .fetch_add(report.failed.len() as u64, Ordering::Relaxed);
    }
}

/// Bring this node onto the mesh: identity, listener, then seeds.
///
/// Returns the resolver even when no seed answered — see the module docs on why
/// an unreachable namespace is a degraded state rather than a fatal one.
///
/// With `log_store` the mesh service's succession log book is restored from it
/// before the listener starts and kept in it afterwards; without it the book is
/// in memory, as it always was. A store that cannot be read is an error.
///
/// The seeds that answered and those that did not are added to `counters`.
pub async fn join_with_application(
    cfg: &NamespaceConfig,
    host: &str,
    application: Option<Arc<dyn spine_mesh::naming_mesh::MeshApplication>>,
    log_store: Option<Arc<dyn LogBookStore>>,
    counters: Arc<NamespaceCounters>,
) -> Result<Arc<MeshNameResolver>> {
    // Held as a `SigningKey`, which wipes itself on drop, not as a bare seed
    // kept alive across the awaits below. `from_seed` takes the seed by value,
    // so one short-lived copy is made for it: ours is wiped, the by-value
    // argument is moved into `spine-mesh` and is out of reach from here.
    let signing = load_or_create_key(&cfg.key_path)?;
    let seed = Zeroizing::new(signing.to_bytes());
    let identity = SigningIdentity::from_seed("spine-core", *seed);
    drop(seed);
    let mesh = Arc::new(MeshNode::new(identity, MeshConfig::default()));

    let (transport, inbound) = TcpNameTransport::with_limits(
        Security::Encrypted(Box::new(EncryptionConfig {
            signing: signing.clone(),
        })),
        cfg.connection_limits(),
    )
    .map_err(|e| anyhow::anyhow!("namespace connection policy: {e}"))?;
    let mut resolver = MeshNameResolver::new(mesh.clone(), transport.clone());
    if let Some(application) = application {
        resolver = resolver.with_application(application);
    }
    if let Some(log_store) = log_store {
        resolver = resolver
            .with_log_store(log_store)
            .map_err(|e| anyhow::anyhow!("namespace succession log book: {e}"))?;
    }
    let resolver = Arc::new(resolver);

    let bind: SocketAddr = format!("{host}:{}", cfg.port)
        .parse()
        .with_context(|| format!("namespace bind address {host}:{}", cfg.port))?;
    let addr = serve_node_with(bind, resolver.clone(), transport, inbound, Some(signing))
        .await
        .map_err(|e| anyhow::anyhow!("namespace listener: {e}"))?;

    // Advertise what the operator configured, falling back to the address we
    // actually bound. The fallback is right for a node on a routable address and
    // wrong behind NAT — which is why an explicit `advertise` exists.
    let advertise = if cfg.advertise.is_empty() {
        vec![addr.to_string()]
    } else {
        cfg.advertise.clone()
    };
    resolver.set_endpoints(advertise).await;

    let seeds = bootstrap_addresses(cfg).await;
    // A seed with a configured key must prove it on every dial, from the very
    // first. The others are trusted on first contact, recorded, and then
    // pinned for the life of the process by the transport. A key that cannot
    // be enforced is a startup error, never silently ignored.
    for (address, key) in cfg.seed_pins()? {
        resolver
            .pin_seed(&address, key)
            .await
            .map_err(|e| anyhow::anyhow!("namespace seed key: {e}"))?;
    }
    let bootstrap = resolver.bootstrap(&seeds).await;
    counters.record_bootstrap(&bootstrap);

    if bootstrap.is_connected() {
        info!(
            listening = %addr,
            seeds_reached = bootstrap.reached.len(),
            peers = bootstrap.peers_after,
            "joined the spine:// namespace"
        );
    } else if seeds.is_empty() {
        info!(listening = %addr, "namespace listener up; no seeds configured");
    } else {
        warn!(
            listening = %addr,
            "no seed answered; serving local names only, not routing"
        );
    }
    for (endpoint, why) in &bootstrap.failed {
        warn!(seed = %endpoint, error = %why, "seed unreachable");
    }

    spawn_maintenance(resolver.clone(), cfg);

    Ok(resolver)
}

pub fn task_application(
    cfg: &NamespaceConfig,
    backend: Arc<dyn spine_storage::StorageBackend>,
) -> Result<Option<Arc<dyn spine_mesh::naming_mesh::MeshApplication>>> {
    if !cfg.tasks_enabled {
        return Ok(None);
    }
    anyhow::ensure!(cfg.enabled, "task worker requires an enabled namespace");
    let allowed = cfg
        .task_peers
        .iter()
        .map(|key| decode_hex(key).context("task_peers entries must be 64 hex characters"))
        .collect::<Result<Vec<_>>>()?;
    let service = Arc::new(spine_core::tasks::TaskService::new(backend, allowed)?);
    service.start();
    Ok(Some(service))
}

/// Configured DNS names supply addresses, never trusted identities. The mesh
/// handshake still verifies every responder. DNS failure leaves local service
/// available and cannot make startup wait indefinitely.
async fn bootstrap_addresses(cfg: &NamespaceConfig) -> Vec<String> {
    use futures::StreamExt;
    let mut seeds = cfg.seed_addresses();
    seeds.truncate(128);
    let results = futures::stream::iter(cfg.dns_seeds.iter().take(16).map(|name| async move {
        match tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::lookup_host(name.as_str()),
        )
        .await
        {
            Ok(Ok(addresses)) => addresses
                .take(16)
                .map(|a| a.to_string())
                .collect::<Vec<_>>(),
            _ => {
                warn!("DNS namespace bootstrap source unavailable");
                Vec::new()
            }
        }
    }))
    .buffered(4)
    .collect::<Vec<_>>()
    .await;
    for address in results.into_iter().flatten() {
        if seeds.len() >= 128 {
            break;
        }
        if !seeds.contains(&address) {
            seeds.push(address);
        }
    }
    seeds
}

/// What one pass of owner renewal found: records re-signed, and names it could
/// not renew because a key succession log has moved them off this node's key.
#[derive(Debug, Default)]
pub struct RenewalPlan {
    /// Records renewed with the node key, ready to publish: those of names
    /// with no succession log whose key is the node key, and those of rotated
    /// names whose current key is the node key (these carry the head claim).
    pub renewed: Vec<spine_name::NameRecord>,
    /// Names (canonical strings) that need renewal but were rotated: the node
    /// key is the name's inception key, which a succession entry has superseded,
    /// so signing with it would produce a record every verifier holding that
    /// entry refuses. Each is reported, never signed.
    pub rotated: Vec<String>,
}

/// The error for a rotated name that owner renewal must not sign.
fn rotated_error(name: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{name} was rotated: this node holds only the name's inception key, which a \
         succession entry has superseded, so it cannot renew the name. Renew it with \
         the name's current key (`spine name renew --log`) and publish the result"
    )
}

/// Owner-driven renewal is opt-in and can never extend a foreign publisher's
/// names. The durable resolver keeps expired sequence markers for recovery.
/// A tombstone is never renewed, and neither is a name this node holds a fresh
/// tombstone for (another persisted row of the same routing key): renewal must
/// not undo a revocation. Renewed records keep their `ttl_secs`, capped at the
/// maximum record lifetime.
///
/// The node holds one key, and renews a name exactly when that key is the one
/// that may sign the name now (spec 02, 10.6 and 10.11):
///
/// - A name with no succession log held, whose key (its inception key) is the
///   node key: renewed with `NameRecord::renewed`, as always.
/// - A rotated name whose log makes the node key its current key: renewed with
///   `NameRecord::renewed_current`, so the record carries the head claim. The
///   name's inception key is then another key, since a log never rotates back
///   to a key the name has used. Only a record the resolver already admits is
///   renewed, and it admits a rotated name's record only under the current
///   key, so this never signs a name the node key has not signed before.
/// - A name whose inception key is the node key and whose log has rotated away
///   from it: not renewed, and reported in [`RenewalPlan::rotated`], since the
///   node key is superseded and only the current key, which this node does not
///   have, may sign.
///
/// A retired or compromised name has no admitted record and is never listed.
pub fn plan_owned_renewals(
    names: &spine_name::LocalResolver,
    signing: &ed25519_dalek::SigningKey,
    now: u64,
    window: u64,
) -> Result<RenewalPlan> {
    let node_key = signing.verifying_key().to_bytes();
    let mut plan = RenewalPlan::default();
    for record in names.published_records()? {
        let Some(inception) = record.name.public_key().copied() else {
            continue;
        };
        // The log held for the record's name, if the name has rotated.
        let state = names
            .succession_state(&inception)
            .filter(|state| state.index() > 0);
        let node_signs = match &state {
            None => inception == node_key,
            Some(state) => state.current() == spine_name::Current::Key(node_key),
        };
        // A name under the node key that has rotated away is reported below.
        let superseded_here = inception == node_key && !node_signs;
        if !node_signs && !superseded_here {
            continue;
        }
        if record.is_tombstone()
            || names.is_revoked(&record.name)
            || record.ttl_secs == 0
            || record.remaining_ttl(now) > window.min((record.ttl_secs as u64) / 2)
        {
            continue;
        }
        let ttl = record.ttl_secs.min(spine_name::MAX_RECORD_TTL_SECS);
        match &state {
            None => plan.renewed.push(record.renewed(signing, now, ttl)?),
            Some(state) if node_signs => {
                plan.renewed
                    .push(record.renewed_current(signing, state, now, ttl)?);
            }
            Some(_) => {
                let name = record.name.to_string();
                if !plan.rotated.contains(&name) {
                    plan.rotated.push(name);
                }
            }
        }
    }
    Ok(plan)
}

/// [`plan_owned_renewals`] as a list of renewed records, failing with a clear
/// error (and renewing nothing) if any name that needs renewal was rotated.
/// The renewal loop uses the plan, so that other names still renew; this form
/// is what the tests assert on.
#[cfg(test)]
pub fn owned_renewals(
    names: &spine_name::LocalResolver,
    signing: &ed25519_dalek::SigningKey,
    now: u64,
    window: u64,
) -> Result<Vec<spine_name::NameRecord>> {
    let plan = plan_owned_renewals(names, signing, now, window)?;
    if let Some(name) = plan.rotated.first() {
        return Err(rotated_error(name));
    }
    Ok(plan.renewed)
}

/// One pass of owner renewal: plan, persist what was renewed (up to `budget`
/// records), publish it to the mesh when there is one. Every outcome is logged
/// and counted in `counters`.
async fn renew_pass(
    names: &spine_name::LocalResolver,
    mesh: Option<&MeshNameResolver>,
    signing: &ed25519_dalek::SigningKey,
    window: u64,
    budget: usize,
    counters: &NamespaceCounters,
) {
    match plan_owned_renewals(names, signing, names.now(), window) {
        Ok(plan) => {
            // Other names still renew; a rotated one is reported every
            // pass until its owner publishes a record signed by its
            // current key.
            for name in &plan.rotated {
                counters
                    .renewals_skipped_rotated
                    .fetch_add(1, Ordering::Relaxed);
                warn!(error = %rotated_error(name), "owner renewal skipped a rotated name");
            }
            for record in plan.renewed.into_iter().take(budget) {
                if let Err(error) = names.publish(record.clone()) {
                    counters.renewals_failed.fetch_add(1, Ordering::Relaxed);
                    warn!(%error, "owner renewal could not be persisted");
                    continue;
                }
                counters.renewals_renewed.fetch_add(1, Ordering::Relaxed);
                if let Some(mesh) = mesh {
                    // A rotated name's record goes out with its log.
                    let log = record
                        .name
                        .public_key()
                        .and_then(|key| names.succession_log(key));
                    let sent = match log {
                        Some(log) => mesh.publish_with_log(record, log).await,
                        None => mesh.publish(record).await,
                    };
                    if let Err(error) = sent {
                        counters
                            .renewals_publish_failed
                            .fetch_add(1, Ordering::Relaxed);
                        warn!(%error, "renewal saved locally but mesh publication failed");
                    }
                }
            }
        }
        Err(error) => {
            counters.renewals_failed.fetch_add(1, Ordering::Relaxed);
            warn!(%error, "owner renewal failed");
        }
    }
}

pub fn spawn_owner_renewal(
    names: Arc<spine_name::LocalResolver>,
    mesh: Option<Arc<MeshNameResolver>>,
    cfg: &NamespaceConfig,
    counters: Arc<NamespaceCounters>,
) -> Result<()> {
    if !cfg.renew_owned_records {
        return Ok(());
    }
    let signing = load_or_create_key(&cfg.key_path)?;
    let window = cfg.lapse_window_secs;
    let period = Duration::from_secs(cfg.renewal_secs.max(1));
    let budget = cfg.maintain_batch.max(1);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            renew_pass(&names, mesh.as_deref(), &signing, window, budget, &counters).await;
        }
    });
    Ok(())
}

/// Keep held records where lookups will find them, for as long as the node runs.
///
/// A record is stored at the K closest nodes *at the time it is published*, and
/// the set of K closest nodes changes underneath it as peers join and leave.
/// Without this the copies stay where they were put while the keyspace position
/// lookups converge on moves away from them, and a name that is still perfectly
/// valid stops being findable.
fn spawn_maintenance(resolver: Arc<MeshNameResolver>, cfg: &NamespaceConfig) {
    let period = Duration::from_secs(cfg.maintain_secs.max(1));
    let policy = MaintenancePolicy {
        lapse_window_secs: cfg.lapse_window_secs,
        max_records: cfg.maintain_batch,
    };

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(period);
        // The first tick fires immediately; skip it. At startup the node has
        // just bootstrapped and has nothing worth re-offering.
        ticker.tick().await;

        loop {
            ticker.tick().await;
            let now = resolver.now();
            let report = resolver.maintain(now, policy).await;

            if report.expired > 0 || report.refreshed > 0 || report.deferred > 0 {
                info!(
                    expired = report.expired,
                    refreshed = report.refreshed,
                    replicas = report.replicas_sent,
                    // Both reported rather than left implicit: `deferred`
                    // above zero for many passes means the budget is below
                    // what this node holds, and `not_ours` climbing means it
                    // is carrying copies lookups will never route to.
                    deferred = report.deferred,
                    not_ours = report.not_ours,
                    "namespace maintenance"
                );
            }
            for name in &report.lapsing {
                // Nothing here can renew it: the expiry is signed into the
                // record, so only whoever holds its key can extend it.
                warn!(name = %name, "name is about to lapse and needs re-signing");
            }
        }
    });
}

/// Whether the key file at `path` can be read or written by anyone but its
/// owner (any group or other permission bit).
#[cfg(unix)]
fn key_file_is_exposed(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o077 != 0)
}

/// Warn when an existing key file is open to other users. Nothing is changed:
/// the operator may have set the mode on purpose. Windows has no mode bits;
/// the file there carries the access list of its directory.
fn warn_if_key_is_exposed(path: &Path) {
    #[cfg(unix)]
    {
        if key_file_is_exposed(path) {
            warn!(
                key = %path.display(),
                "node key file is accessible to other users; restrict it with chmod 600"
            );
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// The signing key of a node-key seed in hex, or `None` if the text is not
/// one. The key zeroizes its secret when dropped (the `zeroize` feature of
/// `ed25519-dalek`, on by default), and the decoded seed array is a
/// [`Zeroizing`] wiped when this function returns.
///
/// This is best effort, not a guarantee: the caller still owns the hex text
/// (see [`load_or_create_key`]), and copies the compiler leaves in registers or
/// stack slots, the allocator, the operating system (page cache, swap, core
/// dumps) and any other crate's types cannot be reached from here.
fn signing_key_from_hex(text: &str) -> Option<ed25519_dalek::SigningKey> {
    let mut seed = Zeroizing::new([0u8; 32]);
    if !decode_hex_into(text, &mut seed) {
        return None;
    }
    Some(ed25519_dalek::SigningKey::from_bytes(&seed))
}

/// Read the node's key, creating one on first run.
///
/// Stored as hex because an operator will need to copy this node's identity into
/// someone else's seed list, and a binary blob makes that needlessly awkward.
/// A new file is created with mode 0600 on Unix; an existing one that other
/// users can read draws a warning.
///
/// The hex text read from the file, the generated seed and the hex text written
/// to it are all held in [`Zeroizing`] and wiped on drop. That is best effort:
/// the buffer `read_to_string` grows may leave earlier, shorter allocations
/// behind, and copies made by the allocator, the operating system or the file
/// system are out of reach. The key file itself is plain text on disk.
pub fn load_or_create_key(path: &str) -> Result<ed25519_dalek::SigningKey> {
    let path = Path::new(path);

    if path.exists() {
        let raw = Zeroizing::new(
            std::fs::read_to_string(path)
                .with_context(|| format!("reading node key {}", path.display()))?,
        );
        let key = signing_key_from_hex(raw.trim())
            .with_context(|| format!("node key {} is not 64 hex characters", path.display()))?;
        warn_if_key_is_exposed(path);
        return Ok(key);
    }

    let seed: Zeroizing<[u8; 32]> = Zeroizing::new(rand::random());
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // Another starter won creation: retain its identity, never overwrite it.
            let raw = Zeroizing::new(std::fs::read_to_string(path)?);
            return signing_key_from_hex(raw.trim())
                .context("existing node key is incomplete or invalid");
        }
        Err(error) => return Err(error).context("creating node identity"),
    };
    use std::io::Write;
    let text = Zeroizing::new(encode_hex(&seed));
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    info!(key = %path.display(), "generated a new node identity");
    Ok(ed25519_dalek::SigningKey::from_bytes(&seed))
}

/// Lowercase hex, built in place: no per-byte temporary strings, so the only
/// copy of the text is the returned one (wrap it in [`Zeroizing`] for a secret).
fn encode_hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

/// Decode 64 hex characters into `out`; false (with `out` unspecified) if the
/// text is not exactly that.
fn decode_hex_into(text: &str, out: &mut [u8; 32]) -> bool {
    if text.len() != 64 {
        return false;
    }
    for (i, slot) in out.iter_mut().enumerate() {
        let Some(byte) = text
            .get(i * 2..i * 2 + 2)
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        else {
            return false;
        };
        *slot = byte;
    }
    true
}

fn decode_hex(text: &str) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    decode_hex_into(text, &mut out).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mesh book writes to namespaces of its own, reads back what it wrote,
    /// and reports a row it cannot decode instead of failing the load.
    #[test]
    fn the_mesh_log_book_uses_its_own_namespaces() {
        use spine_storage::StorageBackend;
        let backend = Arc::new(spine_storage::InMemoryBackend::new());
        let store = Arc::new(BackendLogBookStore::new(backend.clone()));
        let signer = ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]);
        let inception = signer.verifying_key().to_bytes();
        let reveal = ed25519_dalek::SigningKey::from_bytes(&[2u8; 32])
            .verifying_key()
            .to_bytes();
        let next = ed25519_dalek::SigningKey::from_bytes(&[3u8; 32])
            .verifying_key()
            .to_bytes();
        let log = spine_name::SuccessionLog::default()
            .rotated(&inception, &signer, &reveal, &next, 1_000)
            .unwrap();

        store.save_log(&inception, &log).unwrap();
        assert!(backend
            .get(MESH_SUCCESSIONS_NAMESPACE, &inception)
            .unwrap()
            .is_some());
        assert!(backend
            .get("spine-name-successions-v1", &inception)
            .unwrap()
            .is_none());
        backend
            .put(MESH_SUCCESSIONS_NAMESPACE, b"short", b"{}")
            .unwrap();
        backend
            .put(MESH_EQUIVOCATIONS_NAMESPACE, &[9u8; 32], b"not json")
            .unwrap();

        let mut service = spine_mesh::naming::NameService::new(spine_name::NameKey::of(b"x"));
        let report = service.restore_log_book(store.clone()).unwrap();
        assert_eq!((report.logs, report.proofs, report.dropped), (1, 0, 2));
        assert_eq!(service.logs().log(&inception), Some(&log));
        // The rows the store could not decode are gone, so the next start does
        // not count them again; the good row stays.
        assert!(backend
            .get(MESH_SUCCESSIONS_NAMESPACE, b"short")
            .unwrap()
            .is_none());
        assert!(backend
            .get(MESH_EQUIVOCATIONS_NAMESPACE, &[9u8; 32])
            .unwrap()
            .is_none());
        let mut restarted = spine_mesh::naming::NameService::new(spine_name::NameKey::of(b"x"));
        let report = restarted.restore_log_book(store.clone()).unwrap();
        assert_eq!((report.logs, report.proofs, report.dropped), (1, 0, 0));

        store.remove_log(&inception).unwrap();
        assert!(backend
            .get(MESH_SUCCESSIONS_NAMESPACE, &inception)
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_bootstrap_adds_the_seeds_that_answered_and_those_that_did_not() {
        let counters = NamespaceCounters::default();
        counters.record_bootstrap(&BootstrapReport {
            failed: vec![
                ("a:1".into(), "refused".into()),
                ("b:2".into(), "timeout".into()),
            ],
            ..Default::default()
        });
        let snapshot = counters.snapshot();
        assert_eq!((snapshot.seeds_reached, snapshot.seeds_failed), (0, 2));
        counters.record_bootstrap(&BootstrapReport::default());
        let snapshot = counters.snapshot();
        assert_eq!((snapshot.seeds_reached, snapshot.seeds_failed), (0, 2));
    }

    #[tokio::test]
    async fn a_renewal_pass_counts_what_it_renewed_and_what_it_skipped() {
        use spine_name::{NameRecord, SpineUri, SuccessionLog};
        let inception = ed25519_dalek::SigningKey::from_bytes(&[1; 32]);
        let current = ed25519_dalek::SigningKey::from_bytes(&[2; 32]);
        let next = ed25519_dalek::SigningKey::from_bytes(&[3; 32]);
        let other = ed25519_dalek::SigningKey::from_bytes(&[4; 32]);
        let names = spine_name::LocalResolver::persistent(
            || 200,
            Arc::new(spine_storage::InMemoryBackend::new()),
        )
        .unwrap();
        let counters = NamespaceCounters::default();

        // A name of the node key that is due: renewed, with no mesh to tell.
        let mut mine = NameRecord::new(SpineUri::did(other.verifying_key().to_bytes()), 1, 100)
            .unwrap()
            .with_ttl(50);
        mine.sign(&other).unwrap();
        names.publish(mine).unwrap();
        renew_pass(&names, None, &other, 25, 8, &counters).await;
        let snapshot = counters.snapshot();
        assert_eq!(
            (snapshot.renewals_renewed, snapshot.renewals_failed),
            (1, 0)
        );
        assert_eq!(
            (
                snapshot.renewals_publish_failed,
                snapshot.renewals_skipped_rotated
            ),
            (0, 0)
        );

        // A name of the node's inception key that has rotated away: skipped.
        let mut rotated =
            NameRecord::new(SpineUri::did(inception.verifying_key().to_bytes()), 1, 100)
                .unwrap()
                .with_ttl(50);
        rotated.sign(&inception).unwrap();
        names.publish(rotated).unwrap();
        let log = SuccessionLog::default()
            .rotated(
                &inception.verifying_key().to_bytes(),
                &inception,
                &current.verifying_key().to_bytes(),
                &next.verifying_key().to_bytes(),
                150,
            )
            .unwrap();
        names.offer_log(log.clone()).unwrap();
        let state = log.verify(&inception.verifying_key().to_bytes()).unwrap();
        let mut fresh =
            NameRecord::new(SpineUri::did(inception.verifying_key().to_bytes()), 2, 160)
                .unwrap()
                .with_ttl(50);
        fresh.sign_current(&current, &state).unwrap();
        names.publish(fresh).unwrap();
        renew_pass(&names, None, &inception, 25, 8, &counters).await;
        let snapshot = counters.snapshot();
        assert_eq!(snapshot.renewals_skipped_rotated, 1);
        assert_eq!(snapshot.renewals_renewed, 1, "nothing was signed");
        assert_eq!(snapshot.renewals_failed, 0);
    }

    #[test]
    fn hex_round_trips() {
        let seed = [7u8; 32];
        assert_eq!(decode_hex(&encode_hex(&seed)), Some(seed));
    }

    #[test]
    fn a_truncated_or_non_hex_key_is_rejected_not_padded() {
        assert_eq!(decode_hex("abcd"), None);
        assert_eq!(decode_hex(&"z".repeat(64)), None);
    }

    /// The property the whole file exists for: the same path yields the same
    /// identity, so a restart keeps the node's place in the keyspace.
    #[test]
    fn a_key_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("spine-key-{}", std::process::id()));
        let path = dir.join("node.key");
        let path = path.to_str().unwrap();

        let first = load_or_create_key(path).unwrap();
        let second = load_or_create_key(path).unwrap();
        assert_eq!(
            first.to_bytes(),
            second.to_bytes(),
            "a restart must not move the node"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Create, then reload from the file: the same public key comes back, and
    /// the file holds exactly the 64 hex characters of the seed.
    #[test]
    fn a_created_key_reloads_with_the_same_public_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("node.key");
        let path = path.to_str().unwrap();

        let created = load_or_create_key(path).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert_eq!(text.len(), 64);
        assert_eq!(text, encode_hex(&created.to_bytes()));

        let reloaded = load_or_create_key(path).unwrap();
        assert_eq!(created.verifying_key(), reloaded.verifying_key());
    }

    /// A new key file is private to its owner, and a file left wider is
    /// detected (the start-up warning is keyed on this).
    #[cfg(unix)]
    #[test]
    fn a_new_key_file_is_private_and_a_wider_one_is_detected() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("spine-key-mode-{}", std::process::id()));
        let path = dir.join("node.key");
        load_or_create_key(path.to_str().unwrap()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(!key_file_is_exposed(&path));

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(key_file_is_exposed(&path));
        // Loading still works; it warns and changes nothing.
        load_or_create_key(path.to_str().unwrap()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn dns_bootstrap_is_bounded_and_deduplicates_addresses() {
        let cfg = NamespaceConfig {
            seeds: vec!["127.0.0.1:9440".into()],
            dns_seeds: vec!["127.0.0.1:9440".into()],
            ..Default::default()
        };
        assert_eq!(bootstrap_addresses(&cfg).await, vec!["127.0.0.1:9440"]);
    }

    #[test]
    fn renewal_extends_only_records_owned_by_the_configured_key() {
        use spine_name::{NameRecord, SpineUri};
        let signing = ed25519_dalek::SigningKey::from_bytes(&[1; 32]);
        let foreign = ed25519_dalek::SigningKey::from_bytes(&[2; 32]);
        let names = spine_name::LocalResolver::persistent(
            || 200,
            Arc::new(spine_storage::InMemoryBackend::new()),
        )
        .unwrap();
        for key in [&signing, &foreign] {
            let mut record = NameRecord::new(SpineUri::did(key.verifying_key().to_bytes()), 1, 100)
                .unwrap()
                .with_ttl(50);
            record.sign(key).unwrap();
            names.publish(record).unwrap();
        }
        let renewed = owned_renewals(&names, &signing, 200, 25).unwrap();
        assert_eq!(renewed.len(), 1);
        assert_eq!(renewed[0].seq, 2);
        assert!(!renewed[0].is_expired(200));
        assert_eq!(
            renewed[0].name.authority(),
            &spine_name::Authority::Did(signing.verifying_key().to_bytes())
        );
    }

    #[test]
    fn renewal_refuses_a_rotated_name_instead_of_signing_with_a_superseded_key() {
        use spine_name::{NameRecord, SpineUri, SuccessionLog};
        let inception = ed25519_dalek::SigningKey::from_bytes(&[1; 32]);
        let current = ed25519_dalek::SigningKey::from_bytes(&[2; 32]);
        let next = ed25519_dalek::SigningKey::from_bytes(&[3; 32]);
        let other = ed25519_dalek::SigningKey::from_bytes(&[4; 32]);
        let names = spine_name::LocalResolver::persistent(
            || 200,
            Arc::new(spine_storage::InMemoryBackend::new()),
        )
        .unwrap();

        // Another name this node owns, with the same key as its inception key.
        let mut kept = NameRecord::new(SpineUri::did(inception.verifying_key().to_bytes()), 1, 100)
            .unwrap()
            .with_ttl(50);
        kept.sign(&inception).unwrap();
        // A name some other key owns is never this node's business.
        let mut foreign = NameRecord::new(SpineUri::did(other.verifying_key().to_bytes()), 1, 100)
            .unwrap()
            .with_ttl(50);
        foreign.sign(&other).unwrap();
        names.publish(foreign).unwrap();
        names.publish(kept).unwrap();
        assert_eq!(
            owned_renewals(&names, &inception, 200, 25).unwrap().len(),
            1
        );

        // The name rotates: the owner holds the current key, the node does not.
        let log = SuccessionLog::default()
            .rotated(
                &inception.verifying_key().to_bytes(),
                &inception,
                &current.verifying_key().to_bytes(),
                &next.verifying_key().to_bytes(),
                150,
            )
            .unwrap();
        names.offer_log(log.clone()).unwrap();
        let state = log.verify(&inception.verifying_key().to_bytes()).unwrap();
        let mut fresh =
            NameRecord::new(SpineUri::did(inception.verifying_key().to_bytes()), 2, 160)
                .unwrap()
                .with_ttl(50);
        fresh.sign_current(&current, &state).unwrap();
        names.publish(fresh).unwrap();

        let plan = plan_owned_renewals(&names, &inception, 200, 25).unwrap();
        assert!(
            plan.renewed.is_empty(),
            "nothing is signed with a superseded key"
        );
        assert_eq!(plan.rotated.len(), 1);
        let error = owned_renewals(&names, &inception, 200, 25).unwrap_err();
        assert!(error.to_string().contains("was rotated"), "{error}");
        assert!(
            error.to_string().contains("spine name renew --log"),
            "{error}"
        );
    }

    #[test]
    fn renewal_signs_a_rotated_name_when_the_node_key_is_its_current_key() {
        use spine_name::{NameRecord, SpineUri, SuccessionLog, SUCCESSION_META_KEY};
        let inception = ed25519_dalek::SigningKey::from_bytes(&[1; 32]);
        let node = ed25519_dalek::SigningKey::from_bytes(&[2; 32]);
        let next = ed25519_dalek::SigningKey::from_bytes(&[3; 32]);
        let after = ed25519_dalek::SigningKey::from_bytes(&[4; 32]);
        let bystander = ed25519_dalek::SigningKey::from_bytes(&[5; 32]);
        let name_key = inception.verifying_key().to_bytes();
        let node_key = node.verifying_key().to_bytes();
        let names = spine_name::LocalResolver::persistent(
            || 200,
            Arc::new(spine_storage::InMemoryBackend::new()),
        )
        .unwrap();

        // The name was created under another key and rotated to the node key.
        let log = SuccessionLog::default()
            .rotated(
                &name_key,
                &inception,
                &node_key,
                &next.verifying_key().to_bytes(),
                150,
            )
            .unwrap();
        let state = log.verify(&name_key).unwrap();
        let mut record = NameRecord::new(SpineUri::did(name_key), 2, 160)
            .unwrap()
            .with_ttl(50);
        record.sign_current(&node, &state).unwrap();
        names.publish_with_log(record.clone(), log.clone()).unwrap();

        // The holder of the superseded inception key is told, and signs nothing;
        // a key that has nothing to do with the name sees nothing.
        let plan = plan_owned_renewals(&names, &inception, 200, 25).unwrap();
        assert!(plan.renewed.is_empty());
        assert_eq!(plan.rotated, [record.name.to_string()]);
        let plan = plan_owned_renewals(&names, &bystander, 200, 25).unwrap();
        assert!(plan.renewed.is_empty() && plan.rotated.is_empty());

        // The node key is current: it renews, with the head claim.
        let plan = plan_owned_renewals(&names, &node, 200, 25).unwrap();
        assert!(plan.rotated.is_empty());
        assert_eq!(plan.renewed.len(), 1);
        let renewed = plan.renewed[0].clone();
        assert_eq!(renewed.name, record.name);
        assert_eq!(renewed.seq, 3);
        assert_eq!(renewed.published_at, 200);
        assert!(!renewed.is_expired(200));
        assert!(renewed.meta.contains_key(SUCCESSION_META_KEY));
        assert_eq!(
            renewed.meta.get(SUCCESSION_META_KEY),
            record.meta.get(SUCCESSION_META_KEY)
        );
        renewed.verify_with(&node_key).unwrap();
        assert!(
            renewed.verify().is_err(),
            "not signed by the key in the name"
        );
        // What the renewal loop does with it: the resolver admits it.
        names.publish(renewed).unwrap();
        assert!(plan_owned_renewals(&names, &node, 200, 25)
            .unwrap()
            .renewed
            .is_empty());

        // The name rotates on: the node key is superseded, the record it signed
        // is no longer admitted, and nothing is signed with it again.
        let longer = log
            .rotated(
                &name_key,
                &node,
                &next.verifying_key().to_bytes(),
                &after.verifying_key().to_bytes(),
                170,
            )
            .unwrap();
        names.offer_log(longer).unwrap();
        let plan = plan_owned_renewals(&names, &node, 240, 25).unwrap();
        assert!(plan.renewed.is_empty());
        assert!(plan.rotated.is_empty());
    }

    #[test]
    fn renewal_does_not_resurrect_a_revoked_name_and_stays_within_the_maximum_ttl() {
        use spine_name::{NameRecord, SpineUri};
        let signing = ed25519_dalek::SigningKey::from_bytes(&[1; 32]);
        let name = SpineUri::did(signing.verifying_key().to_bytes());
        let names = spine_name::LocalResolver::persistent(
            || 200,
            Arc::new(spine_storage::InMemoryBackend::new()),
        )
        .unwrap();

        // A record at the maximum lifetime renews at the maximum, not above.
        let mut record = NameRecord::new(name.clone(), 1, 100)
            .unwrap()
            .with_ttl(spine_name::MAX_RECORD_TTL_SECS);
        record.sign(&signing).unwrap();
        names.publish(record).unwrap();
        let later = 100 + u64::from(spine_name::MAX_RECORD_TTL_SECS) - 10;
        let renewed = owned_renewals(&names, &signing, later, 25).unwrap();
        assert_eq!(renewed.len(), 1);
        assert!(renewed[0].ttl_secs <= spine_name::MAX_RECORD_TTL_SECS);

        // Once the name is tombstoned, nothing is renewed.
        let mut tombstone = NameRecord::tombstone(name, 2, 150).unwrap();
        tombstone.sign(&signing).unwrap();
        names.publish(tombstone).unwrap();
        assert!(names.is_revoked(&SpineUri::did(signing.verifying_key().to_bytes())));
        assert!(owned_renewals(&names, &signing, 200, u64::MAX)
            .unwrap()
            .is_empty());
    }
}
