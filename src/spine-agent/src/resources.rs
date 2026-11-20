//! Resolution followed by bounded transfer and end-to-end content validation.
use crate::AgentClient;
use anyhow::{ensure, Context, Result};
use spine_name::{content_hash, NameRecord};
use spine_protocol::{
    resource::{ResourceChunk, MAX_RESOURCE_BYTES, MAX_RESOURCE_CHUNK},
    BrowserCommand, NameResolution,
};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

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
            record.content_hash == Some(content_hash(&data)),
            "content hash does not match record"
        );
        let response = self
            .send_request(BrowserCommand::PublishResource {
                record: serde_json::to_value(record)?,
                content_type: content_type.into(),
                data,
            })
            .await?;
        if let Some(error) = response.error {
            anyhow::bail!("publication: {error}");
        }
        response.result.context("publication returned no result")
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
            let response = tokio::time::timeout(
                Duration::from_secs(30),
                self.send_request(BrowserCommand::ReadResource {
                    hash,
                    offset: data.len() as u64,
                    max_bytes: MAX_RESOURCE_CHUNK,
                }),
            )
            .await
            .context("resource transfer timeout")??;
            if let Some(error) = response.error {
                anyhow::bail!("resource transfer: {error}");
            }
            let chunk: ResourceChunk =
                serde_json::from_value(response.result.context("missing resource chunk")?)?;
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
        ensure!(
            content_hash(&data) == hash,
            "resource integrity check failed"
        );
        Ok(RetrievedResource {
            record,
            data,
            content_type: content_type.unwrap_or_default(),
        })
    }

    /// Discover an origin and retrieve a verified representation. TCP is the
    /// portable bootstrap transport here; callers needing transport secrecy use
    /// `connect_tls` and `read_resource` with the same resolved record.
    pub async fn fetch_resource(&mut self, name: &str) -> Result<RetrievedResource> {
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
            let origin = tokio::time::timeout(
                Duration::from_secs(10),
                AgentClient::<TcpStream>::connect(&endpoint.address),
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
        anyhow::bail!("no TCP origin completed transfer: {}", failures.join("; "))
    }
}
