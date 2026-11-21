//! # SPINE Mesh
//!
//! The production agent-mesh stack: Ed25519 agent identities ([`identity`]),
//! the signed mesh envelope and node ([`mesh`]), the signed name-record layer
//! ([`naming`]) and its mesh RPC ([`naming_mesh`]), and the authenticated TCP
//! and WebSocket transports ([`mesh_tcp`], [`mesh_ws`]), the QUIC transport
//! ([`mesh_quic`]) and the per-message signed frame ([`signed_frame`]).
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
pub mod signed_frame;

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
