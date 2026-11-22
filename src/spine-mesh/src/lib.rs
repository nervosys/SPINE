//! # SPINE Mesh
//!
//! The production agent-mesh stack: Ed25519 agent identities ([`identity`]),
//! the signed mesh envelope and node ([`mesh`]), the signed name-record layer
//! ([`naming`]) and its mesh RPC ([`naming_mesh`]), and the authenticated TCP
//! and WebSocket transports ([`mesh_tcp`], [`mesh_ws`]) and the QUIC transport
//! ([`mesh_quic`]).
//!
//! `spine-protocol` is used without its `research` feature, so no research
//! crate is in this crate's dependency tree. `spine-agentic` re-exports
//! everything here, so existing
//! `spine_agentic::mesh::..` paths keep working.

// Allow dead code for extensive public API surface designed for future use
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub mod identity;
pub mod mesh;
pub mod mesh_quic;
pub mod mesh_tcp;
pub mod mesh_ws;
pub mod naming;
pub mod naming_mesh;

/// Unique identifier for an agent in the agentic web
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub Uuid);

impl AgentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }

    /// The agent id a public key determines: the first 16 bytes of
    /// SHA-256(`"spine-agent-id-v1\0"` || key).
    ///
    /// Every identity that signs mesh envelopes ([`identity::SigningIdentity`])
    /// has this id, and every handler that learns or uses an (id, key) pair from
    /// the network requires it ([`AgentId::is_derived_from`]), so `envelope.from`
    /// is a function of the key rather than a free label and a claimed pair that
    /// does not match is dropped.
    ///
    /// The id is 128 bits, so a collision between two keys costs about 2^64
    /// work (birthday bound). That is acceptable because the id is a routing
    /// label and authentication is always against the full 32-byte key:
    /// signatures verify under the key, pins hold keys, and an `Application`
    /// caller is the signing key, never the id. A collision lets an attacker give
    /// two keys of its own the same label; it does not let a key pass as another
    /// key.
    pub fn from_public_key(key: &[u8; 32]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"spine-agent-id-v1\0");
        hasher.update(key);
        let digest = hasher.finalize();
        let mut id = [0u8; 16];
        id.copy_from_slice(&digest[..16]);
        Self(Uuid::from_bytes(id))
    }

    /// Whether this id is [`AgentId::from_public_key`] of `key`.
    pub fn is_derived_from(&self, key: &[u8; 32]) -> bool {
        *self == Self::from_public_key(key)
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let result = hasher.finalize();
        Self(Uuid::from_slice(&result[..16]).unwrap_or_else(|_| Uuid::new_v4()))
    }
}

impl Default for AgentId {
    fn default() -> Self {
        Self::new()
    }
}
