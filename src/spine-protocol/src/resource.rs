//! Bounded, content-addressed resource transfer. Resolution remains a separate
//! operation; offsets allow callers to stop between chunks without an upload or
//! download queue remaining active on the server.
use ed25519_dalek::{Signature, Signer, VerifyingKey};
use serde::{Deserialize, Serialize};

pub use crate::identify::SigningKey;

/// The domain separator at the start of the message a `delete_resource` proof
/// signs: the text, then a zero byte. No other signed object of SPINE uses it.
pub const DELETE_RESOURCE_DOMAIN: &[u8] = b"spine-wire-delete-resource-v1\x00";

/// The message a `delete_resource` proof signs:
///
/// ```text
/// "spine-wire-delete-resource-v1" || 0x00 || hash (32 bytes) || binding (32 bytes)
/// ```
///
/// `binding` is `HELLO_ACK.binding` of the connection the command is sent on,
/// so a proof made for one connection does not verify on another.
pub fn delete_resource_message(hash: &[u8; 32], binding: &[u8; 32]) -> Vec<u8> {
    let mut message = Vec::with_capacity(DELETE_RESOURCE_DOMAIN.len() + 64);
    message.extend_from_slice(DELETE_RESOURCE_DOMAIN);
    message.extend_from_slice(hash);
    message.extend_from_slice(binding);
    message
}

/// The proof `signing` gives for deleting the resource `hash` on the
/// connection whose `HELLO_ACK.binding` is `binding`.
pub fn sign_delete_proof(signing: &SigningKey, hash: &[u8; 32], binding: &[u8; 32]) -> [u8; 64] {
    signing
        .sign(&delete_resource_message(hash, binding))
        .to_bytes()
}

/// Whether `proof` is a valid signature by `owner` over the delete message for
/// `hash` and `binding`. Strict Ed25519 verification, as `IDENTIFY` uses
/// (`VerifyingKey::verify_strict`: RFC 8032 with `S` below the group order, a
/// small-order key or `R` refused). A key that is not a valid point is false.
pub fn verify_delete_proof(
    owner: &[u8; 32],
    hash: &[u8; 32],
    binding: &[u8; 32],
    proof: &[u8; 64],
) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(owner) else {
        return false;
    };
    key.verify_strict(
        &delete_resource_message(hash, binding),
        &Signature::from_bytes(proof),
    )
    .is_ok()
}

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

    fn owner_key() -> SigningKey {
        SigningKey::from_bytes(&[0x21; 32])
    }

    #[test]
    fn the_delete_message_has_a_fixed_layout() {
        let message = delete_resource_message(&[0xaa; 32], &[0xbb; 32]);
        assert_eq!(message.len(), 30 + 32 + 32);
        assert_eq!(&message[..30], b"spine-wire-delete-resource-v1\x00");
        assert_eq!(&message[30..62], &[0xaa; 32]);
        assert_eq!(&message[62..], &[0xbb; 32]);
    }

    #[test]
    fn a_delete_proof_verifies_only_for_its_key_hash_and_binding() {
        let key = owner_key();
        let owner = key.verifying_key().to_bytes();
        let proof = sign_delete_proof(&key, &[1; 32], &[2; 32]);
        assert!(verify_delete_proof(&owner, &[1; 32], &[2; 32], &proof));
        // Another connection, another blob, another key, a changed signature.
        assert!(!verify_delete_proof(&owner, &[1; 32], &[3; 32], &proof));
        assert!(!verify_delete_proof(&owner, &[9; 32], &[2; 32], &proof));
        let other = SigningKey::from_bytes(&[0x22; 32])
            .verifying_key()
            .to_bytes();
        assert!(!verify_delete_proof(&other, &[1; 32], &[2; 32], &proof));
        let mut bad = proof;
        bad[0] ^= 1;
        assert!(!verify_delete_proof(&owner, &[1; 32], &[2; 32], &bad));
    }

    #[test]
    fn a_small_order_owner_never_verifies() {
        // The all-zero key (the store's `LOCAL_OWNER`) is a small-order point:
        // strict verification refuses it whatever the signature.
        assert!(!verify_delete_proof(&[0; 32], &[1; 32], &[2; 32], &[0; 64]));
    }

    #[test]
    fn the_delete_commands_are_closed_maps_with_byte_strings() {
        let command = crate::BrowserCommand::DeleteResource {
            hash: [1; 32],
            owner: [2; 32],
            proof: [3; 64],
        };
        let bytes = cbor::to_vec(&command).unwrap();
        // a1, "delete_resource", a3, "hash", bytes(32) ...
        assert_eq!(bytes[0], 0xa1);
        assert_eq!(bytes[1], 0x6f);
        assert_eq!(&bytes[2..17], b"delete_resource");
        assert_eq!(bytes[17], 0xa3);
        let back: crate::BrowserCommand = cbor::from_slice(&bytes).unwrap();
        assert_eq!(back, command);
        let knowledge = crate::BrowserCommand::DeleteKnowledge { key: "k".into() };
        let bytes = cbor::to_vec(&knowledge).unwrap();
        assert_eq!(
            cbor::from_slice::<crate::BrowserCommand>(&bytes).unwrap(),
            knowledge
        );
    }

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
