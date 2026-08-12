//! An authenticated, forward-secret channel for the mesh.
//!
//! The mesh socket layer carries signed envelopes, so records could never be
//! forged over it — but it was plaintext, which leaks the thing agents most
//! need kept private: *what they are looking for*. A passive observer of
//! resolution traffic learns which names an agent resolves and which
//! capabilities it hunts for, and that is a direct read on the agent's task.
//! Confidentiality here is not defence-in-depth; it is the point.
//!
//! ## The construction
//!
//! A signed ephemeral KEM exchange — the classic "sign your ephemeral key"
//! pattern, with ML-KEM-768 (FIPS 203) as the KEM and Ed25519 for authentication.
//! Both are already vetted primitives used elsewhere in this workspace.
//!
//! ```text
//! Initiator                                            Responder
//!   ephemeral (ek, dk) ← ML-KEM-768.generate()
//!   ── InitiatorHello { ek, id_i, sig_i } ──────────────────►
//!                                    verify sig_i over (label ‖ ek ‖ id_i)
//!                                    (ct, ss) ← Encapsulate(ek)
//!   ◄──────────── ResponderFinish { ct, id_r, sig_r } ──────
//!   verify sig_r over (label ‖ H(msg1) ‖ ct ‖ id_r)
//!   ss ← Decapsulate(dk, ct)
//!
//!   transcript = SHA-256(msg1 ‖ msg2)
//!   HKDF-SHA256(salt = transcript, ikm = ss)
//!     ├─ "spine-mesh-i2r" → initiator→responder key
//!     └─ "spine-mesh-r2i" → responder→initiator key
//! ```
//!
//! **Forward secrecy** comes from the KEM keypair being ephemeral per
//! connection: recovering a node's long-term Ed25519 key later reveals nothing
//! about past traffic, because that key only ever signed, never decrypted.
//!
//! **Authentication binds to mesh identity.** The Ed25519 key that signs the
//! handshake is the same key that places a node in the DHT keyspace and signs
//! its name records, so "the peer I dialed" and "the peer whose records I trust"
//! are the same fact. A dialer passes the identity it expects and the handshake
//! fails if the far end cannot prove it.
//!
//! **Session binding.** The responder's signature covers a hash of the
//! initiator's message, which contains a freshly generated ephemeral key, so a
//! recorded `ResponderFinish` cannot be replayed into a different session.
//!
//! ## What this is not
//!
//! A recorded `InitiatorHello` *can* be replayed to a responder, which will
//! complete a handshake with it. This gains an attacker nothing: without the
//! ephemeral decapsulation key it cannot derive the session key, so it cannot
//! send or read a single frame. Eliminating even that would need a third
//! message or a responder-supplied nonce, and neither is worth the round trip.
//!
//! This construction uses standard primitives in a standard arrangement, but it
//! **has not had external cryptographic review**. It is appropriate for a mesh
//! whose payloads are independently signed; it is not a substitute for TLS in a
//! setting where the transport is the only thing standing between an adversary
//! and unauthenticated data.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use ml_kem::kem::{Decapsulate, DecapsulationKey, Encapsulate, EncapsulationKey};
use ml_kem::{Encoded, EncodedSizeUser, KemCore, MlKem768, MlKem768Params};
use rand::rngs::StdRng;
use rand::SeedableRng;
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Domain separator. Changing it makes old and new implementations refuse to
/// talk rather than silently negotiate something neither intended.
pub const PROTOCOL_LABEL: &[u8] = b"spine-mesh-hs-v1";

const LABEL_INITIATOR: &[u8] = b"spine-mesh-hs-v1/initiator";
const LABEL_RESPONDER: &[u8] = b"spine-mesh-hs-v1/responder";
const INFO_I2R: &[u8] = b"spine-mesh-i2r";
const INFO_R2I: &[u8] = b"spine-mesh-r2i";

/// ML-KEM-768 encapsulation key size (FIPS 203).
const EK_LEN: usize = 1184;
/// ML-KEM-768 ciphertext size (FIPS 203).
const CT_LEN: usize = 1088;
const ED25519_PUB: usize = 32;
const ED25519_SIG: usize = 64;

/// Largest handshake message accepted, bounding an allocation before it is made.
pub const MAX_HANDSHAKE_BYTES: usize = 4096;

/// What can go wrong establishing or using a secure channel.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HandshakeError {
    #[error("malformed handshake message")]
    Malformed,

    #[error("unsupported or mismatched protocol label")]
    WrongProtocol,

    #[error("peer signature did not verify")]
    BadSignature,

    #[error("peer identity {got} is not the expected {expected}")]
    WrongPeer { expected: String, got: String },

    #[error("key encapsulation failed")]
    KemFailure,

    #[error("frame failed authentication — corrupted, replayed, or tampered with")]
    BadFrame,
}

fn hex16(bytes: &[u8]) -> String {
    bytes.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

// ───────────────────────────────── Encoding ─────────────────────────────────

fn put_field(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(bytes);
}

/// Read one length-prefixed field, advancing `cursor`.
fn take_field<'a>(buf: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], HandshakeError> {
    if *cursor + 2 > buf.len() {
        return Err(HandshakeError::Malformed);
    }
    let len = u16::from_be_bytes([buf[*cursor], buf[*cursor + 1]]) as usize;
    *cursor += 2;
    if *cursor + len > buf.len() {
        return Err(HandshakeError::Malformed);
    }
    let field = &buf[*cursor..*cursor + len];
    *cursor += len;
    Ok(field)
}

fn transcript_hash(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

fn verifying_key(bytes: &[u8]) -> Result<VerifyingKey, HandshakeError> {
    let arr: [u8; 32] = bytes.try_into().map_err(|_| HandshakeError::Malformed)?;
    VerifyingKey::from_bytes(&arr).map_err(|_| HandshakeError::BadSignature)
}

fn check_signature(vk: &VerifyingKey, message: &[u8], sig: &[u8]) -> Result<(), HandshakeError> {
    let arr: [u8; 64] = sig.try_into().map_err(|_| HandshakeError::Malformed)?;
    vk.verify(message, &Signature::from_bytes(&arr))
        .map_err(|_| HandshakeError::BadSignature)
}

// ──────────────────────────────── Initiator ────────────────────────────────

/// The dialing half of a handshake, awaiting the responder's reply.
///
/// Holds the ephemeral decapsulation key, which is zeroed on drop — the whole
/// forward-secrecy argument rests on that key not outliving the connection.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Initiator {
    dk_bytes: Vec<u8>,
    #[zeroize(skip)]
    hello: Vec<u8>,
    #[zeroize(skip)]
    signing: SigningKey,
}

impl std::fmt::Debug for Initiator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Initiator").finish_non_exhaustive()
    }
}

impl Initiator {
    /// Begin a handshake, returning the state and the bytes to send.
    pub fn start(signing: &SigningKey, seed: u64) -> (Self, Vec<u8>) {
        let mut rng = StdRng::seed_from_u64(seed);
        let (dk, ek) = MlKem768::generate(&mut rng);
        let ek_bytes = ek.as_bytes().to_vec();
        let identity = signing.verifying_key().to_bytes();

        let mut signed = Vec::with_capacity(LABEL_INITIATOR.len() + EK_LEN + ED25519_PUB);
        signed.extend_from_slice(LABEL_INITIATOR);
        signed.extend_from_slice(&ek_bytes);
        signed.extend_from_slice(&identity);
        let sig = signing.sign(&signed).to_bytes();

        let mut hello = Vec::with_capacity(MAX_HANDSHAKE_BYTES);
        put_field(&mut hello, PROTOCOL_LABEL);
        put_field(&mut hello, &ek_bytes);
        put_field(&mut hello, &identity);
        put_field(&mut hello, &sig);

        (
            Self {
                dk_bytes: dk.as_bytes().to_vec(),
                hello: hello.clone(),
                signing: signing.clone(),
            },
            hello,
        )
    }

    /// Consume the responder's reply and derive the session.
    ///
    /// `expected_peer` pins the far end to a known mesh identity. Passing `None`
    /// accepts whoever answers — appropriate only when dialing a bootstrap
    /// address whose identity is genuinely not yet known.
    pub fn finish(
        self,
        reply: &[u8],
        expected_peer: Option<&[u8; 32]>,
    ) -> Result<Session, HandshakeError> {
        if reply.len() > MAX_HANDSHAKE_BYTES {
            return Err(HandshakeError::Malformed);
        }
        let mut cursor = 0;
        let label = take_field(reply, &mut cursor)?;
        if label != PROTOCOL_LABEL {
            return Err(HandshakeError::WrongProtocol);
        }
        let ct = take_field(reply, &mut cursor)?;
        let identity = take_field(reply, &mut cursor)?;
        let sig = take_field(reply, &mut cursor)?;

        if ct.len() != CT_LEN || identity.len() != ED25519_PUB || sig.len() != ED25519_SIG {
            return Err(HandshakeError::Malformed);
        }

        // Pin before verifying: refusing an unexpected peer outright is clearer
        // than verifying a signature we were never going to accept.
        if let Some(expected) = expected_peer {
            if identity != expected.as_slice() {
                return Err(HandshakeError::WrongPeer {
                    expected: hex16(expected),
                    got: hex16(identity),
                });
            }
        }

        let vk = verifying_key(identity)?;
        let mut signed = Vec::with_capacity(LABEL_RESPONDER.len() + 32 + CT_LEN + ED25519_PUB);
        signed.extend_from_slice(LABEL_RESPONDER);
        signed.extend_from_slice(&transcript_hash(&self.hello));
        signed.extend_from_slice(ct);
        signed.extend_from_slice(identity);
        check_signature(&vk, &signed, sig)?;

        let dk_encoded = <Encoded<DecapsulationKey<MlKem768Params>>>::try_from(
            self.dk_bytes.as_slice(),
        )
        .map_err(|_| HandshakeError::KemFailure)?;
        let dk = DecapsulationKey::<MlKem768Params>::from_bytes(&dk_encoded);
        let ct_typed =
            <ml_kem::Ciphertext<MlKem768>>::try_from(ct).map_err(|_| HandshakeError::Malformed)?;
        let ss = dk
            .decapsulate(&ct_typed)
            .map_err(|_| HandshakeError::KemFailure)?;

        let mut shared = [0u8; 32];
        shared.copy_from_slice(ss.as_slice());
        let peer: [u8; 32] = identity.try_into().map_err(|_| HandshakeError::Malformed)?;

        Ok(Session::derive(
            &shared,
            &self.hello,
            reply,
            peer,
            Role::Initiator,
        ))
    }

    /// This side's own mesh identity.
    pub fn identity(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }
}

// ──────────────────────────────── Responder ────────────────────────────────

/// The accepting half. Stateless: one call consumes the hello and produces both
/// the reply and the established session.
pub struct Responder;

/// A completed responder handshake.
#[derive(Debug)]
pub struct Accepted {
    /// Bytes to send back to the initiator.
    pub reply: Vec<u8>,
    /// The established channel.
    pub session: Session,
}

impl Responder {
    /// Answer an initiator's hello.
    pub fn accept(
        signing: &SigningKey,
        hello: &[u8],
        seed: u64,
    ) -> Result<Accepted, HandshakeError> {
        if hello.len() > MAX_HANDSHAKE_BYTES {
            return Err(HandshakeError::Malformed);
        }
        let mut cursor = 0;
        let label = take_field(hello, &mut cursor)?;
        if label != PROTOCOL_LABEL {
            return Err(HandshakeError::WrongProtocol);
        }
        let ek_bytes = take_field(hello, &mut cursor)?;
        let identity = take_field(hello, &mut cursor)?;
        let sig = take_field(hello, &mut cursor)?;

        if ek_bytes.len() != EK_LEN || identity.len() != ED25519_PUB || sig.len() != ED25519_SIG {
            return Err(HandshakeError::Malformed);
        }

        let vk = verifying_key(identity)?;
        let mut signed = Vec::with_capacity(LABEL_INITIATOR.len() + EK_LEN + ED25519_PUB);
        signed.extend_from_slice(LABEL_INITIATOR);
        signed.extend_from_slice(ek_bytes);
        signed.extend_from_slice(identity);
        check_signature(&vk, &signed, sig)?;

        let ek_encoded = <Encoded<EncapsulationKey<MlKem768Params>>>::try_from(ek_bytes)
            .map_err(|_| HandshakeError::KemFailure)?;
        let ek = EncapsulationKey::<MlKem768Params>::from_bytes(&ek_encoded);
        let mut rng = StdRng::seed_from_u64(seed);
        let (ct, ss) = ek
            .encapsulate(&mut rng)
            .map_err(|_| HandshakeError::KemFailure)?;

        let ct_bytes = ct.to_vec();
        let our_identity = signing.verifying_key().to_bytes();

        // Signing over a hash of the initiator's message binds this reply to
        // this session — a recorded reply cannot be replayed into another.
        let mut to_sign = Vec::with_capacity(LABEL_RESPONDER.len() + 32 + CT_LEN + ED25519_PUB);
        to_sign.extend_from_slice(LABEL_RESPONDER);
        to_sign.extend_from_slice(&transcript_hash(hello));
        to_sign.extend_from_slice(&ct_bytes);
        to_sign.extend_from_slice(&our_identity);
        let our_sig = signing.sign(&to_sign).to_bytes();

        let mut reply = Vec::with_capacity(MAX_HANDSHAKE_BYTES);
        put_field(&mut reply, PROTOCOL_LABEL);
        put_field(&mut reply, &ct_bytes);
        put_field(&mut reply, &our_identity);
        put_field(&mut reply, &our_sig);

        let mut shared = [0u8; 32];
        shared.copy_from_slice(ss.as_slice());
        let peer: [u8; 32] = identity.try_into().map_err(|_| HandshakeError::Malformed)?;

        let session = Session::derive(&shared, hello, &reply, peer, Role::Responder);
        Ok(Accepted { reply, session })
    }
}

// ───────────────────────────────── Session ─────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Initiator,
    Responder,
}

/// An established channel: two directional keys and their frame counters.
///
/// Directions get separate keys so their nonce counters can never collide,
/// which is the failure that turns AES-GCM from secure into catastrophic.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Session {
    tx_key: [u8; 32],
    rx_key: [u8; 32],
    #[zeroize(skip)]
    tx_counter: u64,
    #[zeroize(skip)]
    rx_counter: u64,
    #[zeroize(skip)]
    peer: [u8; 32],
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("peer", &hex16(&self.peer))
            .field("sent", &self.tx_counter)
            .field("received", &self.rx_counter)
            .finish_non_exhaustive()
    }
}

impl Session {
    fn derive(
        shared: &[u8; 32],
        msg1: &[u8],
        msg2: &[u8],
        peer: [u8; 32],
        role: Role,
    ) -> Self {
        // Salting with the full transcript means any disagreement about what was
        // exchanged produces different keys, and the connection simply fails to
        // decrypt rather than proceeding on a mismatched view.
        let mut transcript = Vec::with_capacity(msg1.len() + msg2.len());
        transcript.extend_from_slice(msg1);
        transcript.extend_from_slice(msg2);
        let salt = transcript_hash(&transcript);

        let hk = Hkdf::<Sha256>::new(Some(&salt), shared);
        let mut i2r = [0u8; 32];
        let mut r2i = [0u8; 32];
        hk.expand(INFO_I2R, &mut i2r).expect("HKDF expand");
        hk.expand(INFO_R2I, &mut r2i).expect("HKDF expand");

        let (tx_key, rx_key) = match role {
            Role::Initiator => (i2r, r2i),
            Role::Responder => (r2i, i2r),
        };
        Self {
            tx_key,
            rx_key,
            tx_counter: 0,
            rx_counter: 0,
            peer,
        }
    }

    /// The authenticated mesh identity of the far end.
    pub fn peer_identity(&self) -> &[u8; 32] {
        &self.peer
    }

    /// Frames sent and received so far.
    pub fn counters(&self) -> (u64, u64) {
        (self.tx_counter, self.rx_counter)
    }

    /// Encrypt one frame.
    ///
    /// The frame counter is the nonce and is also authenticated as associated
    /// data, so a frame cannot be reordered or replayed into a different
    /// position in the stream without the tag failing.
    pub fn seal(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let counter = self.tx_counter;
        self.tx_counter += 1;

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.tx_key));
        let nonce_bytes = Self::nonce(counter);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: plaintext,
                    aad: &counter.to_be_bytes(),
                },
            )
            .expect("AES-GCM encryption cannot fail with a valid key and nonce");

        let mut out = Vec::with_capacity(8 + ciphertext.len());
        out.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&ciphertext);
        out
    }

    /// Decrypt one frame, enforcing that it is the next one expected.
    pub fn open(&mut self, frame: &[u8]) -> Result<Vec<u8>, HandshakeError> {
        if frame.len() < 8 + 16 {
            return Err(HandshakeError::BadFrame);
        }
        let counter = u64::from_be_bytes(frame[..8].try_into().unwrap());
        // Strict ordering. A stream cipher over a reliable transport has no
        // reason to skip or repeat, so anything out of order is an attack or a
        // bug — either way, not something to silently accept.
        if counter != self.rx_counter {
            return Err(HandshakeError::BadFrame);
        }

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.rx_key));
        let nonce_bytes = Self::nonce(counter);
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: &frame[8..],
                    aad: &counter.to_be_bytes(),
                },
            )
            .map_err(|_| HandshakeError::BadFrame)?;

        self.rx_counter += 1;
        Ok(plaintext)
    }

    fn nonce(counter: u64) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(&counter.to_be_bytes());
        nonce
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    /// Run a full handshake, returning both sides.
    fn establish() -> (Session, Session) {
        let alice = key(1);
        let bob = key(2);
        let (initiator, hello) = Initiator::start(&alice, 42);
        let accepted = Responder::accept(&bob, &hello, 43).unwrap();
        let client = initiator
            .finish(&accepted.reply, Some(&bob.verifying_key().to_bytes()))
            .unwrap();
        (client, accepted.session)
    }

    #[test]
    fn both_sides_agree_on_peer_identity() {
        let (client, server) = establish();
        assert_eq!(client.peer_identity(), &key(2).verifying_key().to_bytes());
        assert_eq!(server.peer_identity(), &key(1).verifying_key().to_bytes());
    }

    #[test]
    fn a_message_survives_a_round_trip_in_both_directions() {
        let (mut client, mut server) = establish();

        let sealed = client.seal(b"resolve spine://did:...");
        assert_eq!(server.open(&sealed).unwrap(), b"resolve spine://did:...");

        let sealed = server.seal(b"here is the record");
        assert_eq!(client.open(&sealed).unwrap(), b"here is the record");
    }

    #[test]
    fn ciphertext_does_not_contain_the_plaintext() {
        let (mut client, _server) = establish();
        let secret = b"spine://cap:medical.diagnosis/";
        let sealed = client.seal(secret);
        assert!(
            !sealed.windows(secret.len()).any(|w| w == secret),
            "the query an agent makes must not be readable on the wire"
        );
    }

    #[test]
    fn many_frames_stream_in_order() {
        let (mut client, mut server) = establish();
        for i in 0..100u32 {
            let msg = format!("frame {i}");
            let sealed = client.seal(msg.as_bytes());
            assert_eq!(server.open(&sealed).unwrap(), msg.as_bytes());
        }
        assert_eq!(client.counters().0, 100);
        assert_eq!(server.counters().1, 100);
    }

    #[test]
    fn every_frame_uses_a_distinct_nonce() {
        // Nonce reuse is the one failure that makes AES-GCM catastrophic.
        let (mut client, _server) = establish();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            let sealed = client.seal(b"identical plaintext");
            assert!(seen.insert(sealed[..8].to_vec()), "nonce reused");
        }
    }

    #[test]
    fn identical_plaintexts_produce_different_ciphertexts() {
        let (mut client, _server) = establish();
        let a = client.seal(b"same");
        let b = client.seal(b"same");
        assert_ne!(a[8..], b[8..], "a counter-based nonce must randomize output");
    }

    #[test]
    fn a_tampered_frame_is_rejected() {
        let (mut client, mut server) = establish();
        let mut sealed = client.seal(b"authentic payload");
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert_eq!(server.open(&sealed), Err(HandshakeError::BadFrame));
    }

    #[test]
    fn a_replayed_frame_is_rejected() {
        let (mut client, mut server) = establish();
        let sealed = client.seal(b"transfer once");
        assert!(server.open(&sealed).is_ok());
        assert_eq!(
            server.open(&sealed),
            Err(HandshakeError::BadFrame),
            "replaying a frame must not succeed a second time"
        );
    }

    #[test]
    fn frames_delivered_out_of_order_are_rejected() {
        let (mut client, mut server) = establish();
        let first = client.seal(b"one");
        let second = client.seal(b"two");
        assert_eq!(server.open(&second), Err(HandshakeError::BadFrame));
        // The stream is still usable in the correct order.
        assert!(server.open(&first).is_ok());
        assert!(server.open(&second).is_ok());
    }

    #[test]
    fn a_truncated_frame_is_rejected_rather_than_panicking() {
        let (mut client, mut server) = establish();
        let sealed = client.seal(b"payload");
        for cut in 0..sealed.len().min(24) {
            assert!(server.open(&sealed[..cut]).is_err());
        }
    }

    #[test]
    fn the_two_directions_use_different_keys() {
        let (mut client, mut server) = establish();
        let from_client = client.seal(b"probe");
        // The client must not be able to open its own frame: if it could, both
        // directions would share a key and their counters would collide.
        assert!(client.open(&from_client).is_err());
        assert!(server.open(&from_client).is_ok());
    }

    #[test]
    fn a_dialer_refuses_a_peer_it_did_not_ask_for() {
        let alice = key(1);
        let impostor = key(9);
        let (initiator, hello) = Initiator::start(&alice, 1);
        let accepted = Responder::accept(&impostor, &hello, 2).unwrap();

        // Alice wanted to reach Bob, not the impostor.
        let err = initiator
            .finish(&accepted.reply, Some(&key(2).verifying_key().to_bytes()))
            .unwrap_err();
        assert!(matches!(err, HandshakeError::WrongPeer { .. }));
    }

    #[test]
    fn an_unpinned_dial_accepts_whoever_answers() {
        // The bootstrap case: dialing an address whose identity is not yet known.
        let (initiator, hello) = Initiator::start(&key(1), 1);
        let accepted = Responder::accept(&key(7), &hello, 2).unwrap();
        let session = initiator.finish(&accepted.reply, None).unwrap();
        assert_eq!(session.peer_identity(), &key(7).verifying_key().to_bytes());
    }

    #[test]
    fn a_forged_initiator_signature_is_rejected() {
        let (_, mut hello) = Initiator::start(&key(1), 1);
        // Corrupt the trailing signature field.
        let last = hello.len() - 1;
        hello[last] ^= 0xFF;
        assert_eq!(
            Responder::accept(&key(2), &hello, 2).unwrap_err(),
            HandshakeError::BadSignature
        );
    }

    #[test]
    fn a_swapped_ephemeral_key_is_rejected() {
        // A man in the middle substituting its own KEM key cannot re-sign it.
        let (_, hello_a) = Initiator::start(&key(1), 1);
        let (_, hello_b) = Initiator::start(&key(3), 2);

        let mut cursor = 0;
        let label = take_field(&hello_a, &mut cursor).unwrap().to_vec();
        let _ek_a = take_field(&hello_a, &mut cursor).unwrap();
        let id_a = take_field(&hello_a, &mut cursor).unwrap().to_vec();
        let sig_a = take_field(&hello_a, &mut cursor).unwrap().to_vec();

        let mut c2 = 0;
        let _ = take_field(&hello_b, &mut c2).unwrap();
        let ek_b = take_field(&hello_b, &mut c2).unwrap().to_vec();

        let mut forged = Vec::new();
        put_field(&mut forged, &label);
        put_field(&mut forged, &ek_b); // someone else's ephemeral key
        put_field(&mut forged, &id_a);
        put_field(&mut forged, &sig_a);

        assert_eq!(
            Responder::accept(&key(2), &forged, 3).unwrap_err(),
            HandshakeError::BadSignature
        );
    }

    #[test]
    fn a_responder_reply_cannot_be_replayed_into_another_session() {
        let bob = key(2);
        // Session one.
        let (_, hello1) = Initiator::start(&key(1), 10);
        let accepted1 = Responder::accept(&bob, &hello1, 11).unwrap();

        // Session two, fresh ephemeral key.
        let (initiator2, _hello2) = Initiator::start(&key(1), 12);

        // Bob's recorded reply from session one must not satisfy session two.
        let err = initiator2
            .finish(&accepted1.reply, Some(&bob.verifying_key().to_bytes()))
            .unwrap_err();
        assert_eq!(err, HandshakeError::BadSignature);
    }

    #[test]
    fn a_wrong_protocol_label_is_refused() {
        let (initiator, _) = Initiator::start(&key(1), 1);
        let mut reply = Vec::new();
        put_field(&mut reply, b"some-other-protocol");
        put_field(&mut reply, &[0u8; CT_LEN]);
        put_field(&mut reply, &[0u8; 32]);
        put_field(&mut reply, &[0u8; 64]);
        assert_eq!(
            initiator.finish(&reply, None).unwrap_err(),
            HandshakeError::WrongProtocol
        );

        let mut hello = Vec::new();
        put_field(&mut hello, b"nope");
        assert_eq!(
            Responder::accept(&key(1), &hello, 1).unwrap_err(),
            HandshakeError::WrongProtocol
        );
    }

    #[test]
    fn malformed_handshakes_are_rejected_rather_than_panicking() {
        // Truncations at every length.
        let (_, hello) = Initiator::start(&key(1), 1);
        for cut in 0..hello.len().min(64) {
            assert!(Responder::accept(&key(2), &hello[..cut], 2).is_err());
        }
        // Oversized input is refused before any parsing.
        assert_eq!(
            Responder::accept(&key(2), &vec![0u8; MAX_HANDSHAKE_BYTES + 1], 2).unwrap_err(),
            HandshakeError::Malformed
        );
        // A field claiming more bytes than remain.
        let mut lying = Vec::new();
        put_field(&mut lying, PROTOCOL_LABEL);
        lying.extend_from_slice(&u16::MAX.to_be_bytes());
        lying.extend_from_slice(b"short");
        assert_eq!(
            Responder::accept(&key(2), &lying, 2).unwrap_err(),
            HandshakeError::Malformed
        );
    }

    #[test]
    fn wrong_sized_fields_are_rejected() {
        let (initiator, _) = Initiator::start(&key(1), 1);
        let mut reply = Vec::new();
        put_field(&mut reply, PROTOCOL_LABEL);
        put_field(&mut reply, &[0u8; 16]); // not a valid ML-KEM ciphertext
        put_field(&mut reply, &[0u8; 32]);
        put_field(&mut reply, &[0u8; 64]);
        assert_eq!(
            initiator.finish(&reply, None).unwrap_err(),
            HandshakeError::Malformed
        );
    }

    #[test]
    fn separate_connections_derive_independent_keys() {
        // Forward secrecy rests on this: two sessions between the same pair of
        // long-term identities must share no key material.
        let (i1, hello1) = Initiator::start(&key(1), 100);
        let a1 = Responder::accept(&key(2), &hello1, 101).unwrap();
        let mut s1 = i1.finish(&a1.reply, None).unwrap();

        let (i2, hello2) = Initiator::start(&key(1), 200);
        let a2 = Responder::accept(&key(2), &hello2, 201).unwrap();
        let mut s2 = i2.finish(&a2.reply, None).unwrap();

        let mut server1 = a1.session;
        let mut server2 = a2.session;

        // Session one's traffic is unreadable to session two, and vice versa.
        let from_one = s1.seal(b"session one traffic");
        assert!(
            server2.open(&from_one).is_err(),
            "one session's key must not open another's traffic"
        );
        let from_two = s2.seal(b"session two traffic");
        assert!(server1.open(&from_two).is_err());

        // Each session still works on its own terms.
        assert_eq!(server1.open(&from_one).unwrap(), b"session one traffic");
        assert_eq!(server2.open(&from_two).unwrap(), b"session two traffic");
    }

    #[test]
    fn an_empty_payload_round_trips() {
        let (mut client, mut server) = establish();
        let sealed = client.seal(b"");
        assert_eq!(server.open(&sealed).unwrap(), b"");
    }

    #[test]
    fn a_large_payload_round_trips() {
        let (mut client, mut server) = establish();
        let big = vec![0xAB; 1 << 20];
        let sealed = client.seal(&big);
        assert_eq!(server.open(&sealed).unwrap(), big);
    }
}
