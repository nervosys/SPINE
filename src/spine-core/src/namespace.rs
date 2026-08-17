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
//! published*, and that set changes as peers come and go — so [`join`] also
//! starts a maintenance task that re-offers held records to whoever is closest
//! now, and drops the ones that have expired.
//!
//! ## Why joining is allowed to fail
//!
//! [`join`] returns the node even when no seed answered. A namespace this node
//! cannot reach is a degraded state, not a broken one: it still serves the names
//! it holds locally, and refusing to start would take a working origin offline
//! because a seed was down. The failure is reported, loudly, and the node runs.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use spine_agentic::identity::SigningIdentity;
use spine_agentic::mesh::{MeshConfig, MeshNode};
use spine_agentic::mesh_tcp::{serve_node, TcpNameTransport};
use spine_agentic::naming_mesh::{MaintenancePolicy, MeshNameResolver};
use tracing::{info, warn};

use crate::config::NamespaceConfig;

/// Bring this node onto the mesh: identity, listener, then seeds.
///
/// Returns the resolver even when no seed answered — see the module docs on why
/// an unreachable namespace is a degraded state rather than a fatal one.
pub async fn join(cfg: &NamespaceConfig, host: &str) -> Result<Arc<MeshNameResolver>> {
    let seed = load_or_create_key(&cfg.key_path)?;
    let identity = SigningIdentity::from_seed("spine-core", seed);
    let mesh = Arc::new(MeshNode::new(identity, MeshConfig::default()));

    let (transport, inbound) = TcpNameTransport::new();
    let resolver = Arc::new(MeshNameResolver::new(mesh.clone(), transport.clone()));

    let bind: SocketAddr = format!("{host}:{}", cfg.port)
        .parse()
        .with_context(|| format!("namespace bind address {host}:{}", cfg.port))?;
    let addr = serve_node(bind, resolver.clone(), transport, inbound)
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

    let seeds = cfg.seed_addresses();
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
    std::fs::write(path, encode_hex(&seed))
        .with_context(|| format!("writing node key {}", path.display()))?;
    restrict(path);
    info!(key = %path.display(), "generated a new node identity");
    Ok(seed)
}

/// Make the key file owner-readable only, where the platform has the concept.
#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

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
}
