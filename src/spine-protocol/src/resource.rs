//! Bounded, content-addressed resource transfer. Resolution remains a separate
//! operation; offsets allow callers to stop between chunks without an upload or
//! download queue remaining active on the server.
use serde::{Deserialize, Serialize};

pub const MAX_RESOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_RESOURCE_CHUNK: u32 = 64 * 1024;

/// The `ok` value of a `read_resource` reply. A closed struct; `hash` and
/// `data` are CBOR byte strings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResourceChunk {
    #[serde(with = "crate::wirefmt::byte_array")]
    pub hash: [u8; 32],
    pub offset: u64,
    pub total_bytes: u64,
    pub content_type: String,
    #[serde(with = "crate::wirefmt::bytes")]
    pub data: Vec<u8>,
    pub complete: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cbor, frame};

    /// The largest upload is one `publish_resource` body. With byte strings it
    /// is the 8 MiB of data plus a small envelope, so it fits the 16 MiB frame
    /// with a wide margin; an integer array would have taken roughly twice the
    /// data (two bytes for each byte above 23) and would not.
    #[test]
    fn maximum_upload_fits_the_native_frame_limit() {
        let command = crate::BrowserCommand::PublishResource {
            record: serde_json::json!({}),
            content_type: "application/octet-stream".into(),
            data: vec![255; MAX_RESOURCE_BYTES],
        };
        let bytes = cbor::to_vec(&command).unwrap();
        assert!(
            bytes.len() < MAX_RESOURCE_BYTES + 1024,
            "bytes must use CBOR byte strings: {} bytes",
            bytes.len()
        );
        // The envelope is a small constant: the body fits the frame limit
        // (the frame header is 15 bytes, C.1) with room to spare.
        assert!(bytes.len() + frame::HEADER_LEN < frame::MAX_FRAME as usize);
        assert!(
            frame::MAX_FRAME as usize - bytes.len() > 7 * 1024 * 1024,
            "the limit should leave real headroom for the encoding"
        );
        let decoded: crate::BrowserCommand = cbor::from_slice(&bytes).unwrap();
        let crate::BrowserCommand::PublishResource { data, .. } = decoded else {
            panic!("wrong command")
        };
        assert_eq!(data.len(), MAX_RESOURCE_BYTES);
    }

    /// The reply to a `read_resource` is a chunk whose bytes are a byte string
    /// too: a 64 KiB chunk costs 64 KiB plus a small envelope.
    #[test]
    fn a_chunk_costs_its_bytes_plus_an_envelope() {
        let chunk = ResourceChunk {
            hash: [3; 32],
            offset: 65_536,
            total_bytes: 1 << 20,
            content_type: "text/plain".into(),
            data: vec![200; MAX_RESOURCE_CHUNK as usize],
            complete: false,
        };
        let body = crate::connection::response_ok_body(&chunk).unwrap();
        assert!(
            body.len() < MAX_RESOURCE_CHUNK as usize + 160,
            "{}",
            body.len()
        );
        let reply: crate::Reply<ResourceChunk> = cbor::from_slice(&body).unwrap();
        assert_eq!(reply.into_result().unwrap(), chunk);
    }
}
