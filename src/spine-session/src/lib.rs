//! # SPINE Session
//!
//! The authenticated, forward-secret session handshake used by the SPINE mesh:
//! a signed ephemeral ML-KEM-768 exchange with Ed25519 identities, deriving
//! AES-256-GCM session keys. It depends on no other workspace crate.
//!
//! This is custom cryptography that **has not had external cryptographic
//! review**. See the [`handshake`] module documentation for the construction
//! and its stated limits.

pub mod handshake;
