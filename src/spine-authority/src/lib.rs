//! # spine-authority: capability grants
//!
//! The first part of SPINE's authority layer (`docs/spec/03-authority.md`):
//! signed grants, delegation chains that can only narrow, and the eleven-step
//! check that decides whether an invocation is within what was delegated.
//!
//! It is a pure library. It performs no I/O, reads no clock (time is always a
//! parameter) and holds no state: revocation and usage are traits the caller
//! implements ([`RevocationView`], [`UsageLedger`]) and the defaults refuse.
//! The only cryptography is Ed25519 (`verify_strict`) and SHA-256.
//!
//! | Piece | Type |
//! | --- | --- |
//! | A signed statement that a party may act on resources | [`Grant`] |
//! | Root-to-leaf list of grants | [`Chain`] |
//! | The request to be checked | [`Invocation`] |
//! | Issue and delegate | [`Grant::issue_root`], [`Grant::delegate`] |
//! | Narrowing rule | [`is_attenuation_of`], [`Broadening`] |
//! | The eleven steps | [`verify`], [`Denial`] |
//! | Signed revocation, and the feed that carries it | [`Revocation`], [`FeedHead`], [`RevocationFeed`] |
//! | Signed record of what was done | [`Receipt`] |
//! | Atomic in-memory usage ledger | [`MemoryLedger`] |
//!
//! Encodings, bounds and the scope rule are documented on [`grant`],
//! [`invocation`] and [`scope`]. Choices the draft design left open are listed
//! under "Provisional decisions" in `docs/spec/03-authority.md`.
//!
//! This crate is not wired into the node, the mesh, the SDK or the CLI, and it
//! has had no external review.

pub mod attenuation;
mod bytes;
pub mod grant;
pub mod invocation;
pub mod receipt;
pub mod revocation;
pub mod scope;
pub mod state;
pub mod verification;

pub use attenuation::{body_is_attenuation_of, is_attenuation_of, Broadening};
pub use grant::{
    Action, Caveat, Chain, DecodeError, Effect, Grant, GrantBody, GrantId, GrantSpec, IssueError,
    LimitKind, Limits, Malformation, PublicKey, GRANT_DOMAIN, GRANT_ID_DOMAIN, GRANT_VERSION,
    KNOWN_CAVEATS, KNOWN_ROLES, MAX_CAVEATS, MAX_CAVEAT_ARG_LEN, MAX_CAVEAT_NAME_LEN,
    MAX_CHAIN_BYTES, MAX_CHAIN_DEPTH, MAX_GRANT_BYTES, MAX_RESOURCES, MAX_ROLE_LEN,
};
pub use invocation::{
    Invocation, InvocationBody, InvocationId, INVOCATION_DOMAIN, INVOCATION_ID_DOMAIN,
    INVOCATION_VERSION, MAX_INVOCATION_BYTES, MAX_INVOCATION_RESOURCES,
};
pub use receipt::{
    Receipt, ReceiptBody, ReceiptError, MAX_RECEIPT_BYTES, RECEIPT_DOMAIN, RECEIPT_VERSION,
};
pub use revocation::{
    feed_digest, FeedCheck, FeedError, FeedHead, FeedHeadBody, Revocation, RevocationBody,
    RevocationFeed, RevocationId, FEED_DIGEST_DOMAIN, FEED_HEAD_DOMAIN, FEED_HEAD_VERSION,
    MAX_FEED_HEAD_BYTES, MAX_FEED_REVOCATIONS, MAX_REVOCATION_BYTES, REVOCATION_DOMAIN,
    REVOCATION_ID_DOMAIN, REVOCATION_VERSION,
};
pub use scope::{ResourceName, ResourcePattern, ScopeError, MAX_PATH_SEGMENTS, MAX_SEGMENT_LEN};
pub use state::{
    ChargeError, LedgerConfig, MemoryLedger, NoLedger, NoRevocationData, RevocationSet,
    RevocationView, Spent, UsageLedger, RATE_WINDOW_SECS,
};
pub use verification::{
    verify, verify_encoded, Decision, Denial, RevocationBounds, VerifyContext, MAX_CLOCK_SKEW_SECS,
};
