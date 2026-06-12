//! # SPINE — an agentic-first web stack
//!
//! `spine-web` is the umbrella facade for the SPINE workspace. It contains no
//! logic of its own: it re-exports the `spine-*` component crates, each behind a
//! Cargo feature of the same name, so you can depend on one crate and pull in
//! exactly the pieces you need.
//!
//! ```toml
//! # just the wire protocol
//! spine-web = { version = "1.0", default-features = false, features = ["protocol"] }
//!
//! # the agentic-web starter (default): protocol + transport + agent + agentic
//! spine-web = "1.0"
//!
//! # every reusable library
//! spine-web = { version = "1.0", features = ["full"] }
//! ```
//!
//! Each module below is present only when its feature is enabled. Heavy optional
//! backends (`gpu`, `storage`'s SQLite, `k8s`) are opt-in and are *not* part of
//! `full`.
//!
//! The `no_std` crates (`spine-nostd`, `spine-embedded`) are intentionally not
//! re-exported here — depend on them directly for embedded targets, since this
//! facade is a `std` crate.
//!
//! Licensed AGPL-3.0-or-later, with a commercial option (contact
//! <opensource@nervosys.ai>).

#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "protocol")]
#[cfg_attr(docsrs, doc(cfg(feature = "protocol")))]
pub use spine_protocol as protocol;

#[cfg(feature = "transport")]
#[cfg_attr(docsrs, doc(cfg(feature = "transport")))]
pub use spine_transport as transport;

#[cfg(feature = "parser")]
#[cfg_attr(docsrs, doc(cfg(feature = "parser")))]
pub use spine_parser as parser;

#[cfg(feature = "compiler")]
#[cfg_attr(docsrs, doc(cfg(feature = "compiler")))]
pub use spine_compiler as compiler;

#[cfg(feature = "wasm")]
#[cfg_attr(docsrs, doc(cfg(feature = "wasm")))]
pub use spine_wasm as wasm;

#[cfg(feature = "neural")]
#[cfg_attr(docsrs, doc(cfg(feature = "neural")))]
pub use spine_neural as neural;

#[cfg(feature = "crypto")]
#[cfg_attr(docsrs, doc(cfg(feature = "crypto")))]
pub use spine_crypto as crypto;

#[cfg(feature = "agent")]
#[cfg_attr(docsrs, doc(cfg(feature = "agent")))]
pub use spine_agent as agent;

#[cfg(feature = "agentic")]
#[cfg_attr(docsrs, doc(cfg(feature = "agentic")))]
pub use spine_agentic as agentic;

#[cfg(feature = "cluster")]
#[cfg_attr(docsrs, doc(cfg(feature = "cluster")))]
pub use spine_cluster as cluster;

#[cfg(feature = "human")]
#[cfg_attr(docsrs, doc(cfg(feature = "human")))]
pub use spine_human as human;

#[cfg(feature = "stream")]
#[cfg_attr(docsrs, doc(cfg(feature = "stream")))]
pub use spine_stream as stream;

#[cfg(feature = "recursive")]
#[cfg_attr(docsrs, doc(cfg(feature = "recursive")))]
pub use spine_recursive as recursive;

#[cfg(feature = "knowledge")]
#[cfg_attr(docsrs, doc(cfg(feature = "knowledge")))]
pub use spine_knowledge as knowledge;

#[cfg(feature = "storage")]
#[cfg_attr(docsrs, doc(cfg(feature = "storage")))]
pub use spine_storage as storage;

#[cfg(feature = "kernel")]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use spine_kernel as kernel;

#[cfg(feature = "gpu")]
#[cfg_attr(docsrs, doc(cfg(feature = "gpu")))]
pub use spine_gpu as gpu;

#[cfg(feature = "grpc")]
#[cfg_attr(docsrs, doc(cfg(feature = "grpc")))]
pub use spine_grpc as grpc;

#[cfg(feature = "mechgen")]
#[cfg_attr(docsrs, doc(cfg(feature = "mechgen")))]
pub use spine_mechgen as mechgen;

#[cfg(feature = "core")]
#[cfg_attr(docsrs, doc(cfg(feature = "core")))]
pub use spine_core as core;

#[cfg(feature = "cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "cache")))]
pub use spine_cache as cache;

#[cfg(feature = "k8s")]
#[cfg_attr(docsrs, doc(cfg(feature = "k8s")))]
pub use spine_k8s as k8s;
