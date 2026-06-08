//! Per-message signed wire frames.
//!
//! Transport security (TLS/mTLS) authenticates the *channel*; it says nothing
//! about an individual message once it leaves the socket — through a relay, a
//! queue, or a cache. This module wraps a SPINE [`Message`] in a detached
//! **Ed25519** signature computed over its exact binary wire bytes, so any
//! holder can verify, inline and offline:
//!
//! - **integrity** — the bytes were not altered (signature covers the frame),
//! - **authenticity** — they came from the holder of a specific key, and
//! - **non-repudiation** — the signer cannot later disown them.
//!
//! This is a property gRPC/mTLS does not give by default (channel auth, not
//! message auth). The signed bytes are the output of [`wire::encode`], so the
//! CBOR size win still applies underneath the 100-byte signature header.
//!
//! # Envelope layout
//!
//! ```text
//! ┌──────────┬──────────┬──────────┬────────────────────────┐
//! │ "SPS1"   │ pub key  │ signature│ inner wire frame        │
//! │ 4 bytes  │ 32 bytes │ 64 bytes │ N bytes (wire::encode)  │
//! └──────────┴──────────┴──────────┴────────────────────────┘
//! ```
//!
//! The signature is over the inner frame bytes only. A verifier recomputes it,
//! then returns the decoded message together with the signer's public key — the
//! caller decides whether that key is trusted (allow-list, DID resolution, or a
//! match against [`spine_protocol::CapabilityAdvertisement::agent_id`]).

use crate::identity::Ed25519Keypair;
use spine_protocol::wire::{self, WireError};
use spine_protocol::Message;

/// Signed-frame magic: ASCII `"SPS1"` (SPINE Signed v1).
pub const SIGNED_MAGIC: [u8; 4] = *b"SPS1";
/// Ed25519 public-key length.
pub const PUBKEY_LEN: usize = 32;
/// Ed25519 signature length.
pub const SIG_LEN: usize = 64;
/// Total fixed header length (magic + pubkey + signature).
pub const SIGNED_HEADER_LEN: usize = 4 + PUBKEY_LEN + SIG_LEN;

/// Errors from signing or verifying a frame.
#[derive(Debug, thiserror::Error)]
pub enum SignedFrameError {
    /// Buffer shorter than a signed-frame header.
    #[error("signed frame too short: {0} bytes (need >= {SIGNED_HEADER_LEN})")]
    TooShort(usize),
    /// Magic bytes were not `"SPS1"`.
    #[error("bad signed-frame magic")]
    BadMagic,
    /// The Ed25519 signature did not verify against the embedded key.
    #[error("signature verification failed")]
    BadSignature,
    /// The inner wire frame failed to encode/decode.
    #[error("wire codec: {0}")]
    Wire(#[from] WireError),
}

/// A verified message and the key that signed it.
#[derive(Debug, Clone)]
pub struct VerifiedFrame {
    /// The signer's Ed25519 public key (32 bytes). The caller decides trust.
    pub public_key: [u8; PUBKEY_LEN],
    /// The decoded, integrity-checked message.
    pub message: Message,
}

/// Encode `msg` to its wire frame and wrap it in a signature from `keypair`.
pub fn sign_frame(keypair: &Ed25519Keypair, msg: &Message) -> Result<Vec<u8>, SignedFrameError> {
    let inner = wire::encode(msg)?;
    let sig = keypair.sign(&inner);
    let pubkey = keypair.public_key();

    let mut out = Vec::with_capacity(SIGNED_HEADER_LEN + inner.len());
    out.extend_from_slice(&SIGNED_MAGIC);
    out.extend_from_slice(pubkey);
    out.extend_from_slice(&sig);
    out.extend_from_slice(&inner);
    Ok(out)
}

/// Verify a signed frame and decode the inner message.
///
/// Fails closed: a bad magic, a too-short buffer, a forged/altered payload, or
/// an undecodable inner frame all return an error rather than a message.
pub fn verify_frame(buf: &[u8]) -> Result<VerifiedFrame, SignedFrameError> {
    if buf.len() < SIGNED_HEADER_LEN {
        return Err(SignedFrameError::TooShort(buf.len()));
    }
    if buf[0..4] != SIGNED_MAGIC {
        return Err(SignedFrameError::BadMagic);
    }

    let mut public_key = [0u8; PUBKEY_LEN];
    public_key.copy_from_slice(&buf[4..4 + PUBKEY_LEN]);
    let sig = &buf[4 + PUBKEY_LEN..SIGNED_HEADER_LEN];
    let inner = &buf[SIGNED_HEADER_LEN..];

    // Verify the signature over the exact inner wire bytes BEFORE decoding —
    // never hand unauthenticated bytes to the decoder.
    if !Ed25519Keypair::verify(&public_key, inner, sig) {
        return Err(SignedFrameError::BadSignature);
    }

    let message = wire::decode(inner)?;
    Ok(VerifiedFrame {
        public_key,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_then_verify_roundtrips() {
        let kp = Ed25519Keypair::from_seed([7u8; 32]);
        let msg = Message::Ping {
            timestamp: 1_700_000_000,
        };
        let frame = sign_frame(&kp, &msg).unwrap();
        assert_eq!(&frame[0..4], b"SPS1");

        let verified = verify_frame(&frame).unwrap();
        assert_eq!(&verified.public_key, kp.public_key());
        match verified.message {
            Message::Ping { timestamp } => assert_eq!(timestamp, 1_700_000_000),
            other => panic!("expected Ping, got {other:?}"),
        }
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let kp = Ed25519Keypair::from_seed([1u8; 32]);
        let mut frame = sign_frame(&kp, &Message::Ping { timestamp: 1 }).unwrap();
        // Flip a byte in the inner wire frame.
        let last = frame.len() - 1;
        frame[last] ^= 0xFF;
        assert!(matches!(
            verify_frame(&frame),
            Err(SignedFrameError::BadSignature)
        ));
    }

    #[test]
    fn swapped_key_is_rejected() {
        let signer = Ed25519Keypair::from_seed([2u8; 32]);
        let attacker = Ed25519Keypair::from_seed([3u8; 32]);
        let mut frame = sign_frame(&signer, &Message::Ping { timestamp: 9 }).unwrap();
        // Substitute the attacker's public key but keep the signer's signature.
        frame[4..4 + PUBKEY_LEN].copy_from_slice(attacker.public_key());
        assert!(matches!(
            verify_frame(&frame),
            Err(SignedFrameError::BadSignature)
        ));
    }

    #[test]
    fn bad_magic_is_rejected() {
        let kp = Ed25519Keypair::from_seed([4u8; 32]);
        let mut frame = sign_frame(&kp, &Message::Ping { timestamp: 1 }).unwrap();
        frame[0] = b'X';
        assert!(matches!(verify_frame(&frame), Err(SignedFrameError::BadMagic)));
    }

    #[test]
    fn short_buffer_is_rejected() {
        assert!(matches!(
            verify_frame(&[0u8; 10]),
            Err(SignedFrameError::TooShort(10))
        ));
    }
}
