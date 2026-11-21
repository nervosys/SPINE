//! Resolution followed by bounded transfer and end-to-end content validation.
use crate::AgentClient;
use anyhow::{ensure, Context, Result};
use spine_name::{resource_id, verify_resource, NameRecord};
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

#[derive(Debug)]
pub struct RetrievedResource {
    pub record: NameRecord,
    pub content_type: String,
    pub data: Vec<u8>,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> AgentClient<S> {
    pub async fn publish_resource(
        &mut self,
        record: &NameRecord,
        content_type: &str,
        data: Vec<u8>,
    ) -> Result<serde_json::Value> {
        record.verify()?;
        ensure!(data.len() <= MAX_RESOURCE_BYTES, "resource exceeds 8 MiB");
        ensure!(
            record.content_hash == Some(resource_id(content_type, &data)?),
            "resource id does not match record"
        );
        // A refusal is a `NativeError::Remote` with the daemon's code
        // (`record_expired`, `publish_failed`, ...).
        self.send_request(BrowserCommand::PublishResource {
            record: serde_json::to_value(record)?,
            content_type: content_type.into(),
            data,
        })
        .await
    }

    /// Retrieve from an already connected origin, including a TLS-connected
    /// client. A signed record fixes the expected content across every chunk.
    pub async fn read_resource(&mut self, record: NameRecord) -> Result<RetrievedResource> {
        record.verify().context("resource binding signature")?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        ensure!(!record.is_expired(now), "resource binding expired");
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
        let resolution = self
            .resolve_name(name, None)
            .await
            .context("resource resolution")?;
        let NameResolution::Resolved { record, .. } = resolution else {
            anyhow::bail!("resource did not resolve: {resolution:?}");
        };
        let record: NameRecord = serde_json::from_value(record)?;
        record.verify().context("resource binding signature")?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        ensure!(!record.is_expired(now), "resource binding expired");
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
                    Ok(Ok(mut origin)) => match origin.read_resource(record.clone()).await {
                        Ok(resource) => return Ok(resource),
                        Err(error) => failures.push(error.to_string()),
                    },
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
