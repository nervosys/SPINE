//! `IDENTIFY`: a client's proof that it holds an Ed25519 key
//! (`docs/spec/00-wire.md` 6.6).
//!
//! A client may send one `IDENTIFY` frame as the first frame after it has read
//! `HELLO_ACK`. The frame signs a message that covers the exact bodies of the
//! two handshake frames (so the server's per-connection `binding` is in it)
//! and a channel-binding input that is empty in this revision. The server
//! verifies it with strict Ed25519 verification and, on success, records the
//! key as the connection's identity.
//!
//! The identity grants nothing. It proves that some party that holds the
//! private key signed a message made for this connection; it does not prove
//! that the party that signed is the party that is sending the frames, because
//! nothing ties the signature to the byte stream (no channel binding), so a
//! relay that forwards the handshake frames unchanged is not detected.

use ed25519_dalek::{Signature, Signer, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use ed25519_dalek::SigningKey;

use crate::cbor::CborError;
use crate::frame::{Frame, FrameType};
use crate::hello::{decode_body, Close};

/// The domain separator at the start of the signed message: the text, then a
/// zero byte.
pub const IDENTIFY_DOMAIN: &[u8] = b"spine-wire-identify-v1\x00";

/// The longest `channel` the signed message can carry (a SHA-256 or similar
/// value); today the only value a peer accepts is the empty one.
pub const MAX_CHANNEL_LEN: usize = 32;

/// SHA-256 of `bytes`.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// The hashes of the two handshake bodies that `IDENTIFY` signs: `hello` is
/// the SHA-256 of the body bytes of the `HELLO` frame and `ack` the SHA-256 of
/// the body bytes of the `HELLO_ACK` frame, each exactly as it was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transcript {
    /// SHA-256 of the `HELLO` body.
    pub hello: [u8; 32],
    /// SHA-256 of the `HELLO_ACK` body.
    pub ack: [u8; 32],
}

impl Transcript {
    /// The transcript of a handshake whose two bodies are given.
    pub fn of_bodies(hello_body: &[u8], ack_body: &[u8]) -> Transcript {
        Transcript {
            hello: sha256(hello_body),
            ack: sha256(ack_body),
        }
    }
}

/// The message `IDENTIFY` signs:
///
/// ```text
/// "spine-wire-identify-v1" || 0x00
///  || SHA-256(HELLO body) || SHA-256(HELLO_ACK body)
///  || u8(len(channel)) || channel
///  || key
/// ```
///
/// # Panics
///
/// When `channel` is longer than [`MAX_CHANNEL_LEN`]. [`Identify::verify`]
/// refuses a non-empty `channel` before it calls this, so a body from the
/// network cannot reach the panic.
pub fn signed_message(transcript: &Transcript, channel: &[u8], key: &[u8; 32]) -> Vec<u8> {
    assert!(
        channel.len() <= MAX_CHANNEL_LEN,
        "a channel is at most 32 bytes"
    );
    let length = u8::try_from(channel.len()).expect("a channel is at most 32 bytes");
    let mut message = Vec::with_capacity(IDENTIFY_DOMAIN.len() + 32 + 32 + 1 + channel.len() + 32);
    message.extend_from_slice(IDENTIFY_DOMAIN);
    message.extend_from_slice(&transcript.hello);
    message.extend_from_slice(&transcript.ack);
    message.push(length);
    message.extend_from_slice(channel);
    message.extend_from_slice(key);
    message
}

/// The body of an `IDENTIFY` frame: a closed map `{key, sig, channel}`.
///
/// In this revision `channel` MUST be empty and a server refuses any other
/// value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identify {
    /// The client's Ed25519 public key.
    #[serde(with = "crate::wirefmt::byte_array")]
    pub key: [u8; 32],
    /// Ed25519 signature by `key` over [`signed_message`].
    #[serde(with = "crate::wirefmt::byte_array")]
    pub sig: [u8; 64],
    /// The channel-binding input; empty today.
    #[serde(with = "crate::wirefmt::bytes")]
    pub channel: Vec<u8>,
}

impl Identify {
    /// An `IDENTIFY` body signed by `signing` for the connection whose
    /// handshake is `transcript`, with an empty `channel`.
    pub fn sign(signing: &SigningKey, transcript: &Transcript) -> Identify {
        let key = signing.verifying_key().to_bytes();
        let message = signed_message(transcript, &[], &key);
        Identify {
            key,
            sig: signing.sign(&message).to_bytes(),
            channel: Vec::new(),
        }
    }

    /// Encodes this body as an `IDENTIFY` frame with request id 0.
    pub fn to_frame(&self) -> Result<Frame, CborError> {
        Ok(Frame {
            frame_type: FrameType::Identify,
            request_id: 0,
            body: crate::cbor::to_vec(self)?,
        })
    }

    /// Decodes an `IDENTIFY` frame: a closed map of the three keys with the
    /// stated lengths. The error is the `CLOSE` to send. It does not verify
    /// the signature: that needs the handshake ([`Identify::verify`]).
    pub fn from_frame(frame: &Frame) -> Result<Identify, Close> {
        decode_body(frame, FrameType::Identify)
    }

    /// Checks the proof against the connection's handshake and returns the
    /// key it proves. `Err` is the text of the `protocol_violation`.
    ///
    /// The `channel` must be empty; the key must be a valid point; and the
    /// signature must verify under `VerifyingKey::verify_strict` (RFC 8032
    /// with `S < L`, and a small-order key or `R` refused).
    pub fn verify(&self, transcript: &Transcript) -> Result<[u8; 32], String> {
        if !self.channel.is_empty() {
            return Err("IDENTIFY channel must be empty".to_string());
        }
        let key = VerifyingKey::from_bytes(&self.key)
            .map_err(|_| "IDENTIFY key is not a valid Ed25519 public key".to_string())?;
        let message = signed_message(transcript, &self.channel, &self.key);
        let signature = Signature::from_bytes(&self.sig);
        key.verify_strict(&message, &signature)
            .map_err(|_| "IDENTIFY signature does not verify".to_string())?;
        Ok(self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cbor;
    use crate::frame::{decode_frame, encode_frame, CloseCode, MAX_FRAME};

    fn signing() -> SigningKey {
        SigningKey::from_bytes(&[0x11; 32])
    }

    fn transcript() -> Transcript {
        Transcript::of_bodies(b"hello body", b"ack body")
    }

    fn bytes_item(len: usize, fill: u8) -> Vec<u8> {
        // Shortest-form head, as deterministic CBOR requires: one byte below 24.
        let len_byte = u8::try_from(len).unwrap();
        let mut out = if len < 24 {
            vec![0x40 | len_byte]
        } else {
            vec![0x58, len_byte]
        };
        out.resize(out.len() + len, fill);
        out
    }

    #[test]
    fn the_frame_is_133_bytes_with_an_empty_channel() {
        let identify = Identify::sign(&signing(), &transcript());
        let frame = identify.to_frame().unwrap();
        assert_eq!(frame.frame_type, FrameType::Identify);
        assert_eq!(frame.request_id, 0);
        assert_eq!(frame.body.len(), 118);
        let bytes = encode_frame(&frame, MAX_FRAME, false).unwrap();
        assert_eq!(bytes.len(), 133);
        assert_eq!(bytes[..4], [0, 0, 0, 0x81]);
        assert_eq!(bytes[5], 0x06);
        let (back, used) = decode_frame(&bytes, MAX_FRAME).unwrap();
        assert_eq!(used, 133);
        assert_eq!(Identify::from_frame(&back).unwrap(), identify);
    }

    #[test]
    fn keys_are_encoded_key_then_sig_then_channel() {
        let identify = Identify::sign(&signing(), &transcript());
        let body = cbor::to_vec(&identify).unwrap();
        // a3, "key", bytes(32), "sig", bytes(64), "channel", bytes(0).
        assert_eq!(body[0], 0xa3);
        assert_eq!(&body[1..5], &[0x63, b'k', b'e', b'y']);
        assert_eq!(&body[5..7], &[0x58, 0x20]);
        assert_eq!(&body[39..43], &[0x63, b's', b'i', b'g']);
        assert_eq!(&body[43..45], &[0x58, 0x40]);
        assert_eq!(
            &body[109..117],
            &[0x67, b'c', b'h', b'a', b'n', b'n', b'e', b'l']
        );
        assert_eq!(body[117], 0x40);
    }

    #[test]
    fn a_good_proof_verifies_and_names_the_key() {
        let identify = Identify::sign(&signing(), &transcript());
        assert_eq!(
            identify.verify(&transcript()),
            Ok(signing().verifying_key().to_bytes())
        );
    }

    #[test]
    fn the_signature_is_deterministic() {
        assert_eq!(
            Identify::sign(&signing(), &transcript()),
            Identify::sign(&signing(), &transcript())
        );
    }

    #[test]
    fn a_proof_for_another_handshake_does_not_verify() {
        let identify = Identify::sign(&signing(), &transcript());
        let other_ack = Transcript::of_bodies(b"hello body", b"another ack body");
        assert!(identify.verify(&other_ack).is_err());
        let other_hello = Transcript::of_bodies(b"another hello", b"ack body");
        assert!(identify.verify(&other_hello).is_err());
    }

    #[test]
    fn a_changed_key_or_signature_does_not_verify() {
        let good = Identify::sign(&signing(), &transcript());
        let mut bad_sig = good.clone();
        bad_sig.sig[10] ^= 1;
        assert!(bad_sig.verify(&transcript()).is_err());
        let mut bad_key = good.clone();
        bad_key.key = SigningKey::from_bytes(&[0x12; 32])
            .verifying_key()
            .to_bytes();
        assert!(bad_key.verify(&transcript()).is_err());
    }

    #[test]
    fn a_non_empty_channel_is_refused() {
        let mut identify = Identify::sign(&signing(), &transcript());
        identify.channel = vec![0; 32];
        // Even a signature made over that channel is refused: the only legal
        // value today is empty.
        let message = signed_message(&transcript(), &identify.channel, &identify.key);
        identify.sig = signing().sign(&message).to_bytes();
        assert!(identify.verify(&transcript()).is_err());
        identify.channel = vec![0];
        assert!(identify.verify(&transcript()).is_err());
    }

    #[test]
    fn a_small_order_key_is_refused_by_strict_verification() {
        // The identity point (y = 1) is a small-order key, and R = identity
        // with S = 0 is a signature that plain verification accepts for every
        // message. `verify_strict` refuses it.
        let mut key = [0u8; 32];
        key[0] = 1;
        let mut sig = [0u8; 64];
        sig[0] = 1;
        let identify = Identify {
            key,
            sig,
            channel: Vec::new(),
        };
        assert!(identify.verify(&transcript()).is_err());
    }

    #[test]
    fn a_non_canonical_scalar_is_refused() {
        // S + L verifies under plain RFC 8032 and not under the strict rule.
        let good = Identify::sign(&signing(), &transcript());
        let order: [u8; 32] = [
            0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9,
            0xde, 0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
        ];
        let mut sum = [0u8; 32];
        let mut carry = 0u16;
        for ((s, o), out) in good.sig[32..].iter().zip(order.iter()).zip(sum.iter_mut()) {
            let v = u16::from(*s) + u16::from(*o) + carry;
            *out = u8::try_from(v & 0xff).unwrap();
            carry = v >> 8;
        }
        // S < 2^253 and L < 2^253, so S + L < 2^254 and fits in 32 bytes.
        assert_eq!(carry, 0);
        let mut bad = good.clone();
        bad.sig[32..].copy_from_slice(&sum);
        assert!(bad.verify(&transcript()).is_err());
        assert!(good.verify(&transcript()).is_ok());
    }

    fn body_with(key: Vec<u8>, sig: Vec<u8>, channel: Vec<u8>) -> Vec<u8> {
        let mut out = vec![0xa3];
        out.extend([0x63, b'k', b'e', b'y']);
        out.extend(key);
        out.extend([0x63, b's', b'i', b'g']);
        out.extend(sig);
        out.extend([0x67, b'c', b'h', b'a', b'n', b'n', b'e', b'l']);
        out.extend(channel);
        out
    }

    fn violation(body: Vec<u8>) -> Option<CloseCode> {
        let frame = Frame {
            frame_type: FrameType::Identify,
            request_id: 0,
            body,
        };
        Identify::from_frame(&frame)
            .err()
            .and_then(|c| c.close_code())
    }

    #[test]
    fn wrong_lengths_and_types_are_refused() {
        let key = bytes_item(32, 1);
        let sig = bytes_item(64, 2);
        let empty = vec![0x40];
        assert_eq!(
            violation(body_with(key.clone(), sig.clone(), empty.clone())),
            None
        );
        for bad_key in [bytes_item(31, 1), bytes_item(33, 1), vec![0x40], vec![0xf6]] {
            assert_eq!(
                violation(body_with(bad_key, sig.clone(), empty.clone())),
                Some(CloseCode::ProtocolViolation)
            );
        }
        for len in [0usize, 63, 65] {
            assert_eq!(
                violation(body_with(key.clone(), bytes_item(len, 2), empty.clone())),
                Some(CloseCode::ProtocolViolation)
            );
        }
        // A text channel is not bytes.
        assert_eq!(
            violation(body_with(key, sig, vec![0x60])),
            Some(CloseCode::ProtocolViolation)
        );
    }

    #[test]
    fn an_unknown_key_or_a_missing_key_is_refused() {
        let key = bytes_item(32, 1);
        let sig = bytes_item(64, 2);
        let empty = vec![0x40];
        // Four entries; the encoded key of "extra" starts with 0x65, so it
        // sorts after "key" and "sig" (0x63) and before "channel" (0x67).
        let mut body = vec![0xa4];
        body.extend([0x63, b'k', b'e', b'y']);
        body.extend(&key);
        body.extend([0x63, b's', b'i', b'g']);
        body.extend(&sig);
        body.extend([0x65, b'e', b'x', b't', b'r', b'a', 0x01]);
        body.extend([0x67, b'c', b'h', b'a', b'n', b'n', b'e', b'l']);
        body.extend(&empty);
        assert_eq!(violation(body), Some(CloseCode::ProtocolViolation));

        // No channel key.
        let mut body = vec![0xa2];
        body.extend([0x63, b'k', b'e', b'y']);
        body.extend(&key);
        body.extend([0x63, b's', b'i', b'g']);
        body.extend(&sig);
        assert_eq!(violation(body), Some(CloseCode::ProtocolViolation));

        // Not a map.
        assert_eq!(violation(vec![0xf6]), Some(CloseCode::ProtocolViolation));
        assert_eq!(violation(vec![0x80]), Some(CloseCode::ProtocolViolation));
    }

    #[test]
    fn a_frame_of_another_type_is_refused() {
        let frame = Identify::sign(&signing(), &transcript())
            .to_frame()
            .unwrap();
        let wrong = Frame {
            frame_type: FrameType::Request,
            ..frame
        };
        assert_eq!(
            Identify::from_frame(&wrong).unwrap_err().close_code(),
            Some(CloseCode::ProtocolViolation)
        );
    }

    #[test]
    fn the_signed_message_has_the_documented_layout() {
        let t = transcript();
        let key = [9u8; 32];
        let m = signed_message(&t, &[], &key);
        assert_eq!(m.len(), 23 + 32 + 32 + 1 + 32);
        assert_eq!(&m[..22], b"spine-wire-identify-v1");
        assert_eq!(m[22], 0);
        assert_eq!(m[23..55], t.hello);
        assert_eq!(m[55..87], t.ack);
        assert_eq!(m[87], 0);
        assert_eq!(m[88..], key);
        let with_channel = signed_message(&t, &[5u8; 32], &key);
        assert_eq!(with_channel[87], 32);
        assert_eq!(with_channel[88..120], [5u8; 32]);
        assert_eq!(with_channel[120..], key);
    }
}
