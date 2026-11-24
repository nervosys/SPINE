//! Resolution followed by bounded transfer and end-to-end content validation.
use crate::AgentClient;
use anyhow::{ensure, Context, Result};
use spine_name::{
    authorise_record, log_within_bounds, publication_value, resource_id, verify_resource,
    NameError, NameRecord, SuccessionLog, SUCCESSION_META_KEY,
};
use spine_protocol::{
    resource::{ResourceChunk, MAX_RESOURCE_BYTES, MAX_RESOURCE_CHUNK},
    BrowserCommand, NameResolution,
};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

/// Destination policy for `fetch_resource`. Endpoints come from a record the
/// publisher signed, so by default the client refuses to dial loopback,
/// link-local, private, unspecified, multicast and broadcast addresses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FetchPolicy {
    /// Explicit opt-in for callers that really need local or private origins.
    pub allow_private_destinations: bool,
}

impl FetchPolicy {
    /// Permit loopback and private-network origins (tests, local deployments).
    pub fn allow_private() -> Self {
        Self {
            allow_private_destinations: true,
        }
    }

    fn permits(&self, ip: IpAddr) -> bool {
        self.allow_private_destinations || !is_restricted_destination(ip)
    }
}

/// True for addresses a remote publisher must not steer a client toward.
pub fn is_restricted_destination(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_broadcast()
                || v4.octets()[0] == 0
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_restricted_destination(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

/// Final end-to-end check of a downloaded resource against the signed id. The
/// content type came from the serving peer, so it is trusted only through this
/// check: the id commits to both the type and the bytes.
fn check_assembled(hash: &[u8; 32], content_type: &str, data: &[u8]) -> Result<()> {
    verify_resource(hash, content_type, data).context("resource integrity check failed")
}

/// Judge a record that comes with no succession log.
///
/// A record without a `spine.succession` head claim is checked exactly as
/// before, by [`NameRecord::verify`] under the key in the name. A record that
/// carries a head claim is never accepted here: it can only be judged with the
/// name's log, so the answer is [`NameError::NeedSuccessionLog`] (or another
/// refusal for a malformed claim), never a bare bad signature.
pub fn authorise_without_log(record: &NameRecord) -> Result<(), NameError> {
    if record.meta.contains_key(SUCCESSION_META_KEY) {
        authorise_record(record, None)
    } else {
        record.verify()
    }
}

/// Judge a record against the succession log of its name: the log is replayed
/// from the key in the name, and the record must be signed by the key that
/// log makes current and carry the head claim of its state
/// ([`spine_name::authorise_record`]). A log that is over the size bounds or
/// does not verify is refused, as is a record signed by a superseded key.
///
/// An empty log judges the record as the inception key's, so an unrotated name
/// may be sent with an empty log.
pub fn authorise_with_log(record: &NameRecord, log: &SuccessionLog) -> Result<(), NameError> {
    let key = record
        .name
        .public_key()
        .ok_or_else(|| NameError::NotSignable(record.name.to_string()))?;
    if !log_within_bounds(log) {
        return Err(NameError::InvalidSuccessionLog(
            "over the size bounds".to_string(),
        ));
    }
    let state = log
        .verify(key)
        .map_err(|e| NameError::InvalidSuccessionLog(e.to_string()))?;
    authorise_record(record, Some(&state))
}

/// Judge a record with the caller's log if there is one, else without.
fn authorise(record: &NameRecord, log: Option<&SuccessionLog>) -> Result<(), NameError> {
    match log {
        Some(log) => authorise_with_log(record, log),
        None => authorise_without_log(record),
    }
}

/// A refused binding as an error. The need-log refusal is passed through as it
/// is, so its message says what to do; the others keep the context they had.
fn binding_error(error: NameError) -> anyhow::Error {
    match error {
        NameError::NeedSuccessionLog { .. } => anyhow::Error::new(error),
        other => anyhow::Error::new(other).context("resource binding signature"),
    }
}

#[derive(Debug)]
pub struct RetrievedResource {
    pub record: NameRecord,
    pub content_type: String,
    pub data: Vec<u8>,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> AgentClient<S> {
    /// Publish a resource and its binding record. The record is judged under the
    /// key in the name; a record of a rotated name (one that carries a
    /// `spine.succession` head claim) is refused with
    /// [`NameError::NeedSuccessionLog`]: use [`Self::publish_resource_with_log`].
    pub async fn publish_resource(
        &mut self,
        record: &NameRecord,
        content_type: &str,
        data: Vec<u8>,
    ) -> Result<serde_json::Value> {
        authorise_without_log(record)?;
        let value = serde_json::to_value(record)?;
        self.send_resource(record, value, content_type, data).await
    }

    /// [`Self::publish_resource`] for a name with a succession log. The record
    /// is judged against `log` ([`authorise_with_log`]), so a current-key
    /// record of a rotated name is accepted and a superseded-key record is
    /// refused, and the log travels beside the record
    /// ([`spine_name::publication_value`]) for the daemon to judge it too.
    pub async fn publish_resource_with_log(
        &mut self,
        record: &NameRecord,
        log: &SuccessionLog,
        content_type: &str,
        data: Vec<u8>,
    ) -> Result<serde_json::Value> {
        authorise_with_log(record, log)?;
        let value = publication_value(record, log)?;
        self.send_resource(record, value, content_type, data).await
    }

    async fn send_resource(
        &mut self,
        record: &NameRecord,
        value: serde_json::Value,
        content_type: &str,
        data: Vec<u8>,
    ) -> Result<serde_json::Value> {
        ensure!(data.len() <= MAX_RESOURCE_BYTES, "resource exceeds 8 MiB");
        ensure!(
            record.content_hash == Some(resource_id(content_type, &data)?),
            "resource id does not match record"
        );
        // A refusal is a `NativeError::Remote` with the daemon's code
        // (`record_expired`, `publish_failed`, ...).
        self.send_request(BrowserCommand::PublishResource {
            record: value,
            content_type: content_type.into(),
            data,
        })
        .await
    }

    /// Retrieve from an already connected origin, including a TLS-connected
    /// client. A signed record fixes the expected content across every chunk.
    ///
    /// The record is judged under the key in the name; one that carries a
    /// succession head claim is refused with [`NameError::NeedSuccessionLog`]
    /// (use [`Self::read_resource_with_log`]).
    pub async fn read_resource(&mut self, record: NameRecord) -> Result<RetrievedResource> {
        self.read_resource_with_log(record, None).await
    }

    /// [`Self::read_resource`] with the name's succession log, if the caller
    /// has one (`None` is `read_resource`). The daemon's resolution carries no
    /// log, so a client that resolved a rotated name must be given it.
    pub async fn read_resource_with_log(
        &mut self,
        record: NameRecord,
        log: Option<&SuccessionLog>,
    ) -> Result<RetrievedResource> {
        authorise(&record, log).map_err(binding_error)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        ensure!(!record.is_expired(now), "resource binding expired");
        record
            .validate_not_postdated(now)
            .context("resource binding is post-dated")?;
        let hash = record
            .content_hash
            .context("resource has no signed content hash")?;
        let mut data = Vec::new();
        let mut expected_total = None;
        let mut content_type = None;
        loop {
            let chunk: ResourceChunk = tokio::time::timeout(
                Duration::from_secs(30),
                self.send_request(BrowserCommand::ReadResource {
                    hash,
                    offset: data.len() as u64,
                    max_bytes: MAX_RESOURCE_CHUNK,
                }),
            )
            .await
            .context("resource transfer timeout")??;
            ensure!(
                chunk.hash == hash && chunk.offset == data.len() as u64,
                "resource chunk does not match request"
            );
            ensure!(
                chunk.total_bytes <= MAX_RESOURCE_BYTES as u64
                    && chunk.data.len() <= MAX_RESOURCE_CHUNK as usize,
                "resource exceeds transfer bounds"
            );
            ensure!(
                expected_total.is_none_or(|n| n == chunk.total_bytes),
                "resource size changed during transfer"
            );
            ensure!(
                content_type
                    .as_ref()
                    .is_none_or(|t| t == &chunk.content_type),
                "content type changed during transfer"
            );
            expected_total = Some(chunk.total_bytes);
            content_type = Some(chunk.content_type);
            data.extend_from_slice(&chunk.data);
            ensure!(
                data.len() as u64 <= chunk.total_bytes,
                "resource exceeds declared size"
            );
            ensure!(
                chunk.complete == (data.len() as u64 == chunk.total_bytes),
                "incorrect terminal resource chunk"
            );
            if chunk.complete {
                break;
            }
            ensure!(!chunk.data.is_empty(), "resource transfer made no progress");
        }
        // The id commits to the content type as well as the bytes, so a peer
        // that relabels verified bytes fails here.
        let content_type = content_type.unwrap_or_default();
        check_assembled(&hash, &content_type, &data)?;
        Ok(RetrievedResource {
            record,
            data,
            content_type,
        })
    }

    /// Discover an origin and retrieve a verified representation. TCP is the
    /// portable bootstrap transport here; callers needing transport secrecy use
    /// `connect_tls` and `read_resource` with the same resolved record.
    /// Endpoints in loopback, link-local, private and other non-public ranges
    /// are refused; use `fetch_resource_with` and `FetchPolicy::allow_private`
    /// to opt in.
    pub async fn fetch_resource(&mut self, name: &str) -> Result<RetrievedResource> {
        self.fetch_resource_with(name, FetchPolicy::default()).await
    }

    /// `fetch_resource` with an explicit destination policy.
    pub async fn fetch_resource_with(
        &mut self,
        name: &str,
        policy: FetchPolicy,
    ) -> Result<RetrievedResource> {
        self.fetch_resource_with_log(name, policy, None).await
    }

    /// `fetch_resource_with` with the name's succession log, if the caller has
    /// one. The resolution answer carries no log, so without one a rotated
    /// name's record is refused with [`NameError::NeedSuccessionLog`]
    /// before any origin is dialled.
    pub async fn fetch_resource_with_log(
        &mut self,
        name: &str,
        policy: FetchPolicy,
        log: Option<&SuccessionLog>,
    ) -> Result<RetrievedResource> {
        let resolution = self
            .resolve_name(name, None)
            .await
            .context("resource resolution")?;
        let NameResolution::Resolved { record, .. } = resolution else {
            anyhow::bail!("resource did not resolve: {resolution:?}");
        };
        let record: NameRecord = serde_json::from_value(record)?;
        authorise(&record, log).map_err(binding_error)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        ensure!(!record.is_expired(now), "resource binding expired");
        record
            .validate_not_postdated(now)
            .context("resource binding is post-dated")?;
        let mut failures = Vec::new();
        for endpoint in record.endpoints_by_priority() {
            if endpoint.transport != "tcp" {
                continue;
            }
            let resolved = tokio::time::timeout(
                Duration::from_secs(10),
                tokio::net::lookup_host(endpoint.address.as_str()),
            )
            .await;
            let addrs: Vec<SocketAddr> = match resolved {
                Ok(Ok(addrs)) => addrs.collect(),
                Ok(Err(_)) => {
                    failures.push("origin address did not resolve".into());
                    continue;
                }
                Err(_) => {
                    failures.push("origin address resolution timed out".into());
                    continue;
                }
            };
            // Dial only the checked socket addresses, never the hostname.
            let allowed: Vec<SocketAddr> = addrs
                .into_iter()
                .filter(|a| policy.permits(a.ip()))
                .collect();
            if allowed.is_empty() {
                failures.push("origin address refused by destination policy".into());
                continue;
            }
            for addr in allowed {
                let origin = tokio::time::timeout(
                    Duration::from_secs(10),
                    AgentClient::<TcpStream>::connect(&addr.to_string()),
                )
                .await;
                match origin {
                    Ok(Ok(mut origin)) => {
                        match origin.read_resource_with_log(record.clone(), log).await {
                            Ok(resource) => return Ok(resource),
                            Err(error) => failures.push(error.to_string()),
                        }
                    }
                    Ok(Err(_)) => failures.push("origin connection failed".into()),
                    Err(_) => failures.push("origin connection timed out".into()),
                }
            }
        }
        anyhow::bail!("no TCP origin completed transfer: {}", failures.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relabelled_resource_fails_the_final_check() {
        let id = resource_id("text/plain", b"served bytes").unwrap();
        assert!(check_assembled(&id, "text/plain", b"served bytes").is_ok());
        let err = check_assembled(&id, "text/html", b"served bytes").unwrap_err();
        assert_eq!(
            err.downcast_ref::<spine_name::NameError>(),
            Some(&spine_name::NameError::ResourceIdMismatch)
        );
        let err = check_assembled(&id, "", b"served bytes").unwrap_err();
        assert_eq!(
            err.downcast_ref::<spine_name::NameError>(),
            Some(&spine_name::NameError::InvalidContentType)
        );
    }

    /// An unsigned record of an arbitrary name that claims log position 3. The
    /// claim is judged before the signature, so no key is needed.
    fn record_claiming_a_rotation() -> NameRecord {
        let mut record =
            NameRecord::new(spine_name::SpineUri::did([7; 32]), 1, 1_700_000_000).unwrap();
        let claim = spine_name::HeadClaim {
            index: 3,
            head: [9; 32],
        };
        record
            .meta
            .insert(SUCCESSION_META_KEY.to_string(), claim.to_meta_value());
        record
    }

    #[test]
    fn a_head_claim_without_a_log_asks_for_the_log() {
        let record = record_claiming_a_rotation();
        assert_eq!(
            authorise_without_log(&record),
            Err(NameError::NeedSuccessionLog {
                name: record.name.to_string(),
                claimed: 3,
                held: 0,
            })
        );
        // Through the client's error path the message says what is missing.
        let error = binding_error(authorise_without_log(&record).unwrap_err());
        assert!(error.to_string().contains("succession"), "{error}");
        assert!(matches!(
            error.downcast_ref::<NameError>(),
            Some(NameError::NeedSuccessionLog { .. })
        ));
    }

    #[test]
    fn a_record_without_a_claim_is_judged_as_before() {
        // Unsigned: refused exactly as `verify` refuses it, not as a missing log.
        let record = NameRecord::new(spine_name::SpineUri::did([7; 32]), 1, 1_700_000_000).unwrap();
        assert_eq!(authorise_without_log(&record), record.verify());
        assert!(!matches!(
            authorise_without_log(&record),
            Err(NameError::NeedSuccessionLog { .. })
        ));
    }

    #[test]
    fn a_log_that_is_not_the_names_is_refused() {
        let record = record_claiming_a_rotation();
        // An empty log is the inception state: the claim of position 3 is then
        // still a missing entry, not an acceptance.
        assert!(matches!(
            authorise_with_log(&record, &SuccessionLog::default()),
            Err(NameError::NeedSuccessionLog { .. })
        ));
    }

    fn restricted(s: &str) -> bool {
        is_restricted_destination(s.parse().unwrap())
    }

    #[test]
    fn refuses_non_public_destinations() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.1.1",
        ] {
            assert!(restricted(ip), "{ip} should be refused");
        }
    }

    #[test]
    fn accepts_public_destinations_and_opt_in() {
        for ip in [
            "93.184.216.34",
            "8.8.8.8",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(!restricted(ip), "{ip} should be allowed");
        }
        let local: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(!FetchPolicy::default().permits(local));
        assert!(FetchPolicy::allow_private().permits(local));
    }
}
