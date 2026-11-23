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
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use spine_mesh::identity::SigningIdentity;
use spine_mesh::mesh::{MeshConfig, MeshNode};
use spine_mesh::mesh_tcp::{serve_node_with, EncryptionConfig, Security, TcpNameTransport};
use spine_mesh::naming_mesh::{MaintenancePolicy, MeshNameResolver};
use tracing::{info, warn};

use crate::config::NamespaceConfig;

/// Bring this node onto the mesh: identity, listener, then seeds.
///
/// Returns the resolver even when no seed answered — see the module docs on why
/// an unreachable namespace is a degraded state rather than a fatal one.
pub async fn join_with_application(
    cfg: &NamespaceConfig,
    host: &str,
    application: Option<Arc<dyn spine_mesh::naming_mesh::MeshApplication>>,
) -> Result<Arc<MeshNameResolver>> {
    let seed = load_or_create_key(&cfg.key_path)?;
    let identity = SigningIdentity::from_seed("spine-core", seed);
    let mesh = Arc::new(MeshNode::new(identity, MeshConfig::default()));

    let (transport, inbound) = TcpNameTransport::with_limits(
        Security::Encrypted(Box::new(EncryptionConfig {
            signing: ed25519_dalek::SigningKey::from_bytes(&seed),
        })),
        cfg.connection_limits(),
    )
    .map_err(|e| anyhow::anyhow!("namespace connection policy: {e}"))?;
    let mut resolver = MeshNameResolver::new(mesh.clone(), transport.clone());
    if let Some(application) = application {
        resolver = resolver.with_application(application);
    }
    let resolver = Arc::new(resolver);

    let bind: SocketAddr = format!("{host}:{}", cfg.port)
        .parse()
        .with_context(|| format!("namespace bind address {host}:{}", cfg.port))?;
    let addr = serve_node_with(
        bind,
        resolver.clone(),
        transport,
        inbound,
        Some(ed25519_dalek::SigningKey::from_bytes(&seed)),
    )
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
    /// Records renewed with the node key, ready to publish.
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
/// The node holds one key, the key of the names it owns, which is their
/// inception key. A name whose succession log has rotated away from that key is
/// not renewed: it is reported in [`RenewalPlan::rotated`] (spec 02, 10.11),
/// since only the name's current key, which this node never has, may sign it.
pub fn plan_owned_renewals(
    names: &spine_name::LocalResolver,
    signing: &ed25519_dalek::SigningKey,
    now: u64,
    window: u64,
) -> Result<RenewalPlan> {
    let inception = signing.verifying_key().to_bytes();
    let authority = spine_name::Authority::Did(inception);
    let rotated = names
        .succession_state(&inception)
        .is_some_and(|state| state.index() > 0);
    let mut plan = RenewalPlan::default();
    for record in names.published_records()? {
        if record.name.authority() == &authority
            && !record.is_tombstone()
            && !names.is_revoked(&record.name)
            && record.ttl_secs > 0
            && record.remaining_ttl(now) <= window.min((record.ttl_secs as u64) / 2)
        {
            if rotated {
                let name = record.name.to_string();
                if !plan.rotated.contains(&name) {
                    plan.rotated.push(name);
                }
                continue;
            }
            let ttl = record.ttl_secs.min(spine_name::MAX_RECORD_TTL_SECS);
            plan.renewed.push(record.renewed(signing, now, ttl)?);
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

pub fn spawn_owner_renewal(
    names: Arc<spine_name::LocalResolver>,
    mesh: Option<Arc<MeshNameResolver>>,
    cfg: &NamespaceConfig,
) -> Result<()> {
    if !cfg.renew_owned_records {
        return Ok(());
    }
    let signing = ed25519_dalek::SigningKey::from_bytes(&load_or_create_key(&cfg.key_path)?);
    let window = cfg.lapse_window_secs;
    let period = Duration::from_secs(cfg.renewal_secs.max(1));
    let budget = cfg.maintain_batch.max(1);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            match plan_owned_renewals(&names, &signing, names.now(), window) {
                Ok(plan) => {
                    // Other names still renew; a rotated one is reported every
                    // pass until its owner publishes a record signed by its
                    // current key.
                    for name in &plan.rotated {
                        warn!(error = %rotated_error(name), "owner renewal skipped a rotated name");
                    }
                    for record in plan.renewed.into_iter().take(budget) {
                        if let Err(error) = names.publish(record.clone()) {
                            warn!(%error, "owner renewal could not be persisted");
                            continue;
                        }
                        if let Some(mesh) = &mesh {
                            if let Err(error) = mesh.publish(record).await {
                                warn!(%error, "renewal saved locally but mesh publication failed");
                            }
                        }
                    }
                }
                Err(error) => warn!(%error, "owner renewal failed"),
            }
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

/// Read the node's key seed, creating one on first run.
///
/// Stored as hex because an operator will need to copy this node's identity into
/// someone else's seed list, and a binary blob makes that needlessly awkward.
fn load_or_create_key(path: &str) -> Result<[u8; 32]> {
    let path = Path::new(path);

    if path.exists() {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading node key {}", path.display()))?;
        let bytes = decode_hex(raw.trim())
            .with_context(|| format!("node key {} is not 64 hex characters", path.display()))?;
        return Ok(bytes);
    }

    let seed: [u8; 32] = rand::random();
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
            let raw = std::fs::read_to_string(path)?;
            return decode_hex(raw.trim()).context("existing node key is incomplete or invalid");
        }
        Err(error) => return Err(error).context("creating node identity"),
    };
    use std::io::Write;
    file.write_all(encode_hex(&seed).as_bytes())?;
    file.sync_all()?;
    info!(key = %path.display(), "generated a new node identity");
    Ok(seed)
}

fn encode_hex(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn decode_hex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(first, second, "a restart must not move the node");

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
