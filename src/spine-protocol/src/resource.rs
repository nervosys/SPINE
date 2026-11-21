//! Bounded, content-addressed resource transfer. Resolution remains a separate
//! operation; offsets allow callers to stop between chunks without an upload or
//! download queue remaining active on the server.
use serde::{Deserialize, Serialize};

pub const MAX_RESOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_RESOURCE_CHUNK: u32 = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceChunk {
    pub hash: [u8; 32],
    pub offset: u64,
    pub total_bytes: u64,
    pub content_type: String,
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
    pub complete: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maximum_upload_fits_the_native_frame_limit() {
        let command = crate::BrowserCommand::PublishResource {
            record: serde_json::json!({}),
            content_type: "application/octet-stream".into(),
            data: vec![255; MAX_RESOURCE_BYTES],
        };
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&command, &mut bytes).unwrap();
        assert!(
            bytes.len() < MAX_RESOURCE_BYTES + 1024,
            "bytes must use CBOR byte strings"
        );
        let decoded: crate::BrowserCommand = ciborium::de::from_reader(&bytes[..]).unwrap();
        let crate::BrowserCommand::PublishResource { data, .. } = decoded else {
            panic!("wrong command")
        };
        assert_eq!(data.len(), MAX_RESOURCE_BYTES);
    }
}
