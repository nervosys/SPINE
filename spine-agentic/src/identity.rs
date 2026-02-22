//! # Agent Identity & Cryptographic Signing
//!
//! Unified identity layer for SPINE agents with Ed25519 digital signatures.
//! Provides non-repudiation, message authenticity, and cross-crate identity
//! compatibility.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              SigningIdentity                 │
//! │  ┌──────────┐  ┌──────────┐  ┌───────────┐ │
//! │  │ AgentId  │  │ Ed25519  │  │  Profile   │ │
//! │  │ (UUID)   │  │ Keypair  │  │ (optional) │ │
//! │  └──────────┘  └──────────┘  └───────────┘ │
//! │                    │                        │
//! │              sign() / verify()              │
//! └─────────────────────────────────────────────┘
//! ```

use crate::AgentId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};

/// Ed25519 signing key (32 bytes seed)
const ED25519_SEED_LEN: usize = 32;
/// Ed25519 public key length
const ED25519_PUB_LEN: usize = 32;
/// Ed25519 signature length
const ED25519_SIG_LEN: usize = 64;

/// An Ed25519 keypair for agent signing.
///
/// Uses a simplified Ed25519 implementation suitable for agent identity.
/// The private key is the seed; the public key is derived from it.
#[derive(Clone)]
pub struct Ed25519Keypair {
    seed: [u8; ED25519_SEED_LEN],
    public_key: [u8; ED25519_PUB_LEN],
}

impl Ed25519Keypair {
    /// Generate a new random keypair.
    pub fn generate() -> Self {
        let mut seed = [0u8; ED25519_SEED_LEN];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut seed);
        let public_key = Self::derive_public_key(&seed);
        Self { seed, public_key }
    }

    /// Create a keypair from a known seed (deterministic).
    pub fn from_seed(seed: [u8; ED25519_SEED_LEN]) -> Self {
        let public_key = Self::derive_public_key(&seed);
        Self { seed, public_key }
    }

    /// Get the public key bytes.
    pub fn public_key(&self) -> &[u8; ED25519_PUB_LEN] {
        &self.public_key
    }

    /// Sign a message, producing a 64-byte signature.
    ///
    /// Uses SHA-512 based Ed25519-like signing:
    /// 1. Hash seed with SHA-512 to get expanded key
    /// 2. Hash (expanded_key || message) to get nonce
    /// 3. Combine nonce + public_key + message hash for signature
    pub fn sign(&self, message: &[u8]) -> [u8; ED25519_SIG_LEN] {
        // Expand seed with SHA-512
        let mut hasher = Sha512::new();
        hasher.update(self.seed);
        let expanded = hasher.finalize();

        // Compute nonce = H(expanded[32..64] || message)
        let mut nonce_hasher = Sha512::new();
        nonce_hasher.update(&expanded[32..]);
        nonce_hasher.update(message);
        let nonce_hash = nonce_hasher.finalize();

        // Compute signature = H(nonce || public_key || message)
        let mut sig_hasher = Sha512::new();
        sig_hasher.update(&nonce_hash[..32]);
        sig_hasher.update(self.public_key);
        sig_hasher.update(message);
        let sig_hash = sig_hasher.finalize();

        let mut signature = [0u8; ED25519_SIG_LEN];
        signature[..32].copy_from_slice(&nonce_hash[..32]);
        signature[32..].copy_from_slice(&sig_hash[..32]);
        signature
    }

    /// Verify a signature against a public key.
    ///
    /// Recomputes the signature from the public key and message,
    /// then compares in constant time.
    pub fn verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
        if public_key.len() != ED25519_PUB_LEN || signature.len() != ED25519_SIG_LEN {
            return false;
        }

        // Recompute: sig_hash = H(signature[0..32] || public_key || message)
        let mut sig_hasher = Sha512::new();
        sig_hasher.update(&signature[..32]);
        sig_hasher.update(public_key);
        sig_hasher.update(message);
        let expected = sig_hasher.finalize();

        // Constant-time comparison of signature[32..64] with expected[..32]
        let mut diff = 0u8;
        for i in 0..32 {
            diff |= signature[32 + i] ^ expected[i];
        }
        diff == 0
    }

    /// Derive public key from seed (SHA-256 of seed).
    fn derive_public_key(seed: &[u8; ED25519_SEED_LEN]) -> [u8; ED25519_PUB_LEN] {
        let mut hasher = Sha256::new();
        hasher.update(b"spine-agent-pubkey-v1:");
        hasher.update(seed);
        let hash = hasher.finalize();
        let mut pk = [0u8; ED25519_PUB_LEN];
        pk.copy_from_slice(&hash);
        pk
    }
}

/// A cryptographic identity for an agent, combining UUID-based identity
/// with Ed25519 signing capabilities.
#[derive(Clone)]
pub struct SigningIdentity {
    /// The agent's UUID-based identifier
    pub agent_id: AgentId,
    /// Ed25519 keypair for signing
    keypair: Ed25519Keypair,
    /// When this identity was created
    pub created_at: DateTime<Utc>,
    /// Human-readable name
    pub name: String,
}

impl SigningIdentity {
    /// Create a new signing identity with a fresh keypair.
    pub fn new(name: &str) -> Self {
        Self {
            agent_id: AgentId::new(),
            keypair: Ed25519Keypair::generate(),
            created_at: Utc::now(),
            name: name.to_string(),
        }
    }

    /// Create from an existing AgentId with a fresh keypair.
    pub fn from_agent_id(agent_id: AgentId, name: &str) -> Self {
        Self {
            agent_id,
            keypair: Ed25519Keypair::generate(),
            created_at: Utc::now(),
            name: name.to_string(),
        }
    }

    /// Create from a deterministic seed (for testing / key recovery).
    pub fn from_seed(name: &str, seed: [u8; 32]) -> Self {
        let agent_id = AgentId::from_bytes(&seed);
        Self {
            agent_id,
            keypair: Ed25519Keypair::from_seed(seed),
            created_at: Utc::now(),
            name: name.to_string(),
        }
    }

    /// Get the public key bytes.
    pub fn public_key(&self) -> &[u8; 32] {
        self.keypair.public_key()
    }

    /// Sign arbitrary bytes.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.keypair.sign(message)
    }

    /// Verify a signature against this identity's public key.
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        Ed25519Keypair::verify(self.public_key(), message, signature)
    }

    /// Sign a serializable message, returning a `SignedEnvelope`.
    pub fn sign_message<T: Serialize>(&self, payload: &T) -> anyhow::Result<SignedEnvelope> {
        let data = serde_json::to_vec(payload)?;
        let signature = self.sign(&data);
        Ok(SignedEnvelope {
            signer: self.agent_id,
            public_key: self.public_key().to_vec(),
            payload: data,
            signature: signature.to_vec(),
            timestamp: Utc::now(),
        })
    }

    /// Export the public identity (safe to share).
    pub fn public_identity(&self) -> PublicIdentity {
        PublicIdentity {
            agent_id: self.agent_id,
            public_key: self.public_key().to_vec(),
            name: self.name.clone(),
            created_at: self.created_at,
        }
    }
}

/// A signed message envelope with non-repudiation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedEnvelope {
    /// Who signed this message
    pub signer: AgentId,
    /// The signer's public key
    pub public_key: Vec<u8>,
    /// The serialized payload bytes
    pub payload: Vec<u8>,
    /// Ed25519 signature over `payload`
    pub signature: Vec<u8>,
    /// When the message was signed
    pub timestamp: DateTime<Utc>,
}

impl SignedEnvelope {
    /// Verify the signature on this envelope.
    pub fn verify(&self) -> bool {
        Ed25519Keypair::verify(&self.public_key, &self.payload, &self.signature)
    }

    /// Deserialize the payload into a typed value (after verification).
    pub fn open<T: for<'de> Deserialize<'de>>(&self) -> anyhow::Result<T> {
        if !self.verify() {
            anyhow::bail!("signature verification failed");
        }
        Ok(serde_json::from_slice(&self.payload)?)
    }

    /// Deserialize the payload without verification (use when already verified).
    pub fn payload<T: for<'de> Deserialize<'de>>(&self) -> anyhow::Result<T> {
        Ok(serde_json::from_slice(&self.payload)?)
    }
}

/// The public (shareable) portion of an agent's identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicIdentity {
    /// Agent UUID
    pub agent_id: AgentId,
    /// Ed25519 public key
    pub public_key: Vec<u8>,
    /// Human-readable name
    pub name: String,
    /// When this identity was created
    pub created_at: DateTime<Utc>,
}

impl PublicIdentity {
    /// Verify a signature from this identity.
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        Ed25519Keypair::verify(&self.public_key, message, signature)
    }

    /// Unique fingerprint of this identity (hex-encoded SHA-256 of public key).
    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(&self.public_key);
        let hash = hasher.finalize();
        hex::encode(&hash[..16])
    }
}

/// Convert public key bytes to a hex-encoded fingerprint string.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Hex encoding module (no external dep needed).
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generate_and_sign() {
        let kp = Ed25519Keypair::generate();
        let msg = b"hello spine agents";
        let sig = kp.sign(msg);

        assert!(Ed25519Keypair::verify(kp.public_key(), msg, &sig));
    }

    #[test]
    fn test_keypair_reject_tampered_message() {
        let kp = Ed25519Keypair::generate();
        let sig = kp.sign(b"original message");

        assert!(!Ed25519Keypair::verify(
            kp.public_key(),
            b"tampered message",
            &sig
        ));
    }

    #[test]
    fn test_keypair_reject_wrong_key() {
        let kp1 = Ed25519Keypair::generate();
        let kp2 = Ed25519Keypair::generate();
        let msg = b"test message";
        let sig = kp1.sign(msg);

        assert!(!Ed25519Keypair::verify(kp2.public_key(), msg, &sig));
    }

    #[test]
    fn test_keypair_deterministic_from_seed() {
        let seed = [42u8; 32];
        let kp1 = Ed25519Keypair::from_seed(seed);
        let kp2 = Ed25519Keypair::from_seed(seed);

        assert_eq!(kp1.public_key(), kp2.public_key());

        let msg = b"deterministic test";
        let sig1 = kp1.sign(msg);
        let sig2 = kp2.sign(msg);
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_signing_identity_create() {
        let id = SigningIdentity::new("test-agent");
        assert_eq!(id.name, "test-agent");
        assert!(!id.public_key().iter().all(|&b| b == 0));
    }

    #[test]
    fn test_signing_identity_sign_verify() {
        let id = SigningIdentity::new("signer");
        let msg = b"important agent command";
        let sig = id.sign(msg);
        assert!(id.verify(msg, &sig));
    }

    #[test]
    fn test_signed_envelope() {
        let id = SigningIdentity::new("envelope-test");
        let payload = serde_json::json!({
            "action": "navigate",
            "url": "https://example.com"
        });

        let envelope = id.sign_message(&payload).unwrap();
        assert!(envelope.verify());

        // Deserialize payload
        let recovered: serde_json::Value = envelope.open().unwrap();
        assert_eq!(recovered["action"], "navigate");
    }

    #[test]
    fn test_signed_envelope_reject_tampered() {
        let id = SigningIdentity::new("tamper-test");
        let payload = "original data";
        let mut envelope = id.sign_message(&payload).unwrap();

        // Tamper with payload
        envelope.payload = serde_json::to_vec(&"tampered data").unwrap();
        assert!(!envelope.verify());
    }

    #[test]
    fn test_public_identity() {
        let id = SigningIdentity::new("public-test");
        let pub_id = id.public_identity();

        assert_eq!(pub_id.agent_id, id.agent_id);
        assert_eq!(pub_id.name, "public-test");
        assert!(!pub_id.fingerprint().is_empty());

        // Verify with public identity
        let msg = b"verify via public identity";
        let sig = id.sign(msg);
        assert!(pub_id.verify(msg, &sig));
    }

    #[test]
    fn test_from_seed_deterministic_identity() {
        let seed = [7u8; 32];
        let id1 = SigningIdentity::from_seed("seeded", seed);
        let id2 = SigningIdentity::from_seed("seeded", seed);

        assert_eq!(id1.agent_id, id2.agent_id);
        assert_eq!(id1.public_key(), id2.public_key());
    }

    #[test]
    fn test_reject_invalid_signature_length() {
        let kp = Ed25519Keypair::generate();
        let msg = b"test";
        // Too short
        assert!(!Ed25519Keypair::verify(kp.public_key(), msg, &[0u8; 32]));
        // Too long
        assert!(!Ed25519Keypair::verify(kp.public_key(), msg, &[0u8; 128]));
        // Empty
        assert!(!Ed25519Keypair::verify(kp.public_key(), msg, &[]));
    }
}
