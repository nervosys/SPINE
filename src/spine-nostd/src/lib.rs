//! # SPINE no_std Core
//!
//! Minimal core primitives for embedded and WASM targets without the standard
//! library. Provides the essential types and algorithms needed to participate
//! in the SPINE network from constrained environments.
//!
//! ## Features
//!
//! - `alloc`: Enable types that require heap allocation (Vec-backed buffers)
//! - Default (no features): Pure stack-based, zero-allocation primitives
//!
//! ## Modules
//!
//! - [`types`]: Core data types (AgentId, LatentVector, FrameHeader)
//! - [`codec`]: Frame encoding/decoding for wire protocol
//! - [`hash`]: Lightweight hashing (FNV-1a, SipHash-like)
//! - [`math`]: Fixed-point and integer-only math for neural ops

#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod codec;
pub mod hash;
pub mod math;
pub mod types;

pub use codec::{decode_frame_header, encode_frame_header};
pub use hash::{fnv1a_32, fnv1a_64};
pub use math::{cosine_similarity_fixed, dot_product_fixed, softmax_fixed};
pub use types::{AgentIdBytes, FrameHeader, LatentVectorFixed};
