//! Revocation records, the feed head, and the feed that implements step 8.
//!
//! This module defines the objects and how to verify them. It does not move
//! them: how a record or a head reaches a verifier (layer 2, a relay, a file)
//! is outside this crate.
//!
//! # A revocation
//!
//! A [`Revocation`] is a [`RevocationBody`] plus a signature. Both are
//! deterministic CBOR, a map with text keys, byte strings for keys, ids and
//! signatures. The signed bytes are [`REVOCATION_DOMAIN`] followed by the CBOR
//! of the body. The id of a revocation ([`Revocation::id`]) is SHA-256 over
//! [`REVOCATION_ID_DOMAIN`] and the whole encoding.
//!
//! | Key | CBOR type | Meaning |
//! | --- | --- | --- |
//! | `v` | unsigned integer | format version, 1 |
//! | `revoker` | byte string, 32 | the signer, an Ed25519 public key |
//! | `grant` | byte string, 32 | id of the grant being revoked |
//! | `revoked_at` | unsigned integer | when the revoker made the statement, Unix seconds |
//! | `nonce` | byte string, 16 | makes otherwise identical records distinct |
//!
//! Who may revoke: the issuer of the grant, or the issuer of any grant above
//! it in the chain (the root's issuer can revoke everything below it). The
//! record names only the grant id and the revoker key, so the right is
//! checked where the chain is at hand: at step 8, with the chain being
//! verified ([`Revocation::is_authorized_by`]). A record signed by a key that
//! issued no grant at or above the revoked grant in the presented chain is
//! ignored for that chain. A revocation covers the grant and, because a child
//! cites its parent's id and the chain contains the parent, everything
//! delegated from it.
//!
//! # The feed head
//!
//! A [`FeedHead`] is signed by the feed's publisher. It states a sequence
//! number, the time `as_of` which the feed is complete, the number of
//! revocations, and a digest: SHA-256 over [`FEED_DIGEST_DOMAIN`] and the
//! concatenated ids of all revocations, sorted ascending as byte strings.
//! [`RevocationFeed::load`] accepts a set of revocations only together with a
//! head that matches it exactly, and only if the head is newer than the last
//! one the caller saw.
//!
//! | Key | CBOR type | Meaning |
//! | --- | --- | --- |
//! | `v` | unsigned integer | format version, 1 |
//! | `publisher` | byte string, 32 | the signer |
//! | `seq` | unsigned integer | increases with every head the publisher issues |
//! | `as_of` | unsigned integer | the feed is complete as of this Unix time |
//! | `count` | unsigned integer | number of revocations |
//! | `digest` | byte string, 32 | see above |
//!
//! The head detects a set that has lost a record (truncation), gained one the
//! publisher did not list, or been paired with a head from another time
//! (replay, when the caller keeps its last-seen sequence). It does not detect
//! a publisher that leaves a record out of every head, or two verifiers being
//! shown different feeds. The README and the specification say so.

use std::collections::BTreeMap;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spine_cbor::CborError;

use crate::grant::{Chain, DecodeError, GrantId, PublicKey};
use crate::state::RevocationView;

/// The revocation format version this crate reads and writes.
pub const REVOCATION_VERSION: u64 = 1;

/// The feed head format version this crate reads and writes.
pub const FEED_HEAD_VERSION: u64 = 1;

/// Prefix of the signed bytes of a revocation.
pub const REVOCATION_DOMAIN: &[u8] = b"spine-authority-revocation-v1\0";

/// Prefix of the hash that is a revocation's id.
pub const REVOCATION_ID_DOMAIN: &[u8] = b"spine-authority-revocation-id-v1\0";

/// Prefix of the signed bytes of a feed head.
pub const FEED_HEAD_DOMAIN: &[u8] = b"spine-authority-feed-head-v1\0";

/// Prefix of the hash over the sorted revocation ids that a head carries.
pub const FEED_DIGEST_DOMAIN: &[u8] = b"spine-authority-feed-digest-v1\0";

/// Longest encoded revocation, in bytes.
pub const MAX_REVOCATION_BYTES: usize = 512;

/// Longest encoded feed head, in bytes.
pub const MAX_FEED_HEAD_BYTES: usize = 512;

/// Most revocations one feed holds. This bounds the memory of a
/// [`RevocationFeed`] (about 16384 records of a few hundred bytes).
pub const MAX_FEED_REVOCATIONS: usize = 16_384;

/// The id of a revocation: see [`Revocation::id`].
pub type RevocationId = [u8; 32];

type EntryKey = (GrantId, RevocationId);

/// The signed content of a revocation. See the module documentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevocationBody {
    /// Format version.
    #[serde(rename = "v")]
    pub version: u64,
    /// The signer.
    #[serde(with = "crate::bytes::fixed")]
    pub revoker: PublicKey,
    /// The id of the grant being revoked.
    #[serde(with = "crate::bytes::fixed")]
    pub grant: GrantId,
    /// When the statement was made, Unix seconds.
    pub revoked_at: u64,
    /// Makes otherwise identical records distinct.
    #[serde(with = "crate::bytes::fixed")]
    pub nonce: [u8; 16],
}

impl RevocationBody {
    /// The bytes the revoker signs: [`REVOCATION_DOMAIN`] then the
    /// deterministic CBOR of the body.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, CborError> {
        let mut out: Vec<u8> = REVOCATION_DOMAIN.to_vec();
        let body = spine_cbor::to_vec(self)?;
        out.extend_from_slice(body.as_slice());
        Ok(out)
    }
}

/// A signed revocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revocation {
    /// The signed content.
    pub body: RevocationBody,
    /// Ed25519 signature by `body.revoker`.
    #[serde(rename = "sig", with = "crate::bytes::fixed")]
    pub signature: [u8; 64],
}

impl Revocation {
    /// Signs `body` as it is. The signer should be `body.revoker`; if it is
    /// not, the signature does not verify.
    pub fn sign(signer: &SigningKey, body: RevocationBody) -> Result<Revocation, CborError> {
        let message = body.signing_bytes()?;
        let signature: Signature = signer.sign(message.as_slice());
        Ok(Revocation {
            body,
            signature: signature.to_bytes(),
        })
    }

    /// Revokes `grant` as `signer`. There is no clock and no random source,
    /// so `revoked_at` and `nonce` are the caller's.
    pub fn issue(
        signer: &SigningKey,
        grant: GrantId,
        revoked_at: u64,
        nonce: [u8; 16],
    ) -> Result<Revocation, CborError> {
        let body = RevocationBody {
            version: REVOCATION_VERSION,
            revoker: signer.verifying_key().to_bytes(),
            grant,
            revoked_at,
            nonce,
        };
        Revocation::sign(signer, body)
    }

    /// The canonical encoding of the whole record.
    pub fn encode(&self) -> Result<Vec<u8>, CborError> {
        spine_cbor::to_vec(self)
    }

    /// The id of the record: SHA-256 over [`REVOCATION_ID_DOMAIN`] and the
    /// encoding.
    pub fn id(&self) -> Result<RevocationId, CborError> {
        let encoded = self.encode()?;
        let mut hasher = Sha256::new();
        hasher.update(REVOCATION_ID_DOMAIN);
        hasher.update(encoded.as_slice());
        let digest: [u8; 32] = hasher.finalize().into();
        Ok(digest)
    }

    /// Decodes a record: exactly the canonical encoding, within
    /// [`MAX_REVOCATION_BYTES`]. The version and the signature are not checked
    /// here; see [`Revocation::verify`].
    pub fn decode(bytes: &[u8]) -> Result<Revocation, DecodeError> {
        if bytes.len() > MAX_REVOCATION_BYTES {
            return Err(DecodeError::TooLarge(MAX_REVOCATION_BYTES));
        }
        let record: Revocation = spine_cbor::from_slice(bytes).map_err(DecodeError::Cbor)?;
        let again = spine_cbor::to_vec(&record).map_err(DecodeError::Cbor)?;
        if again.as_slice() != bytes {
            return Err(DecodeError::NonCanonical);
        }
        Ok(record)
    }

    /// Whether the signature is a valid strict Ed25519 signature by `revoker`.
    pub fn verify_signature(&self) -> bool {
        let key = match VerifyingKey::from_bytes(&self.body.revoker) {
            Ok(key) => key,
            Err(_) => return false,
        };
        let message = match self.body.signing_bytes() {
            Ok(message) => message,
            Err(_) => return false,
        };
        let signature = Signature::from_bytes(&self.signature);
        key.verify_strict(message.as_slice(), &signature).is_ok()
    }

    /// Checks the version and the signature. Whether the revoker may revoke
    /// this grant is a separate question, [`Revocation::is_authorized_by`].
    pub fn verify(&self) -> Result<(), FeedError> {
        if self.body.version != REVOCATION_VERSION {
            return Err(FeedError::UnknownVersion);
        }
        if !self.verify_signature() {
            return Err(FeedError::BadRevocationSignature);
        }
        Ok(())
    }

    /// `true` when the revoked grant is in `chain` and the revoker is the
    /// issuer of that grant or of a grant above it in the chain.
    ///
    /// The chain is expected to have passed steps 1 to 7 of verification; this
    /// only reads grant ids and issuers.
    pub fn is_authorized_by(&self, chain: &Chain) -> bool {
        for (index, grant) in chain.grants.iter().enumerate() {
            match grant.id() {
                Ok(id) if id == self.body.grant => return self.is_authorized_at(chain, index),
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        false
    }

    /// The revoker is the issuer of one of the first `index + 1` grants.
    fn is_authorized_at(&self, chain: &Chain, index: usize) -> bool {
        chain
            .grants
            .iter()
            .take(index.saturating_add(1))
            .any(|grant| grant.body.iss == self.body.revoker)
    }
}

/// Why a feed, a head or a revocation was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FeedError {
    /// Encoding failed.
    #[error("cbor: {0}")]
    Cbor(CborError),
    /// A record or the head has an unknown version.
    #[error("unknown version")]
    UnknownVersion,
    /// A revocation's signature does not verify.
    #[error("a revocation signature does not verify")]
    BadRevocationSignature,
    /// The head's signature does not verify.
    #[error("the feed head signature does not verify")]
    BadHeadSignature,
    /// The head is signed by a key other than the trusted publisher.
    #[error("the feed head is not from the trusted publisher")]
    WrongPublisher,
    /// The head's sequence is not above the last one the caller saw.
    #[error("the feed head sequence does not advance")]
    SequenceRollback,
    /// The head claims completeness as of a time after the caller's clock.
    #[error("the feed head is dated in the future")]
    FutureHead,
    /// More revocations than [`MAX_FEED_REVOCATIONS`].
    #[error("the feed has too many revocations")]
    TooMany,
    /// The same revocation appears twice.
    #[error("a revocation appears twice")]
    DuplicateRevocation,
    /// A revocation is dated after the head's `as_of`.
    #[error("a revocation is dated after the head")]
    RevokedAfterHead,
    /// The head's count differs from the number of revocations.
    #[error("the head count does not match the set")]
    CountMismatch,
    /// The head's digest differs from the digest of the set.
    #[error("the head digest does not match the set")]
    DigestMismatch,
}

/// The signed content of a feed head. See the module documentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedHeadBody {
    /// Format version.
    #[serde(rename = "v")]
    pub version: u64,
    /// The publisher, the signer.
    #[serde(with = "crate::bytes::fixed")]
    pub publisher: PublicKey,
    /// Sequence number, strictly increasing per publisher.
    #[serde(rename = "seq")]
    pub sequence: u64,
    /// The feed is complete as of this Unix time (seconds).
    pub as_of: u64,
    /// Number of revocations.
    pub count: u64,
    /// SHA-256 over [`FEED_DIGEST_DOMAIN`] and the sorted revocation ids.
    #[serde(with = "crate::bytes::fixed")]
    pub digest: [u8; 32],
}

impl FeedHeadBody {
    /// The bytes the publisher signs: [`FEED_HEAD_DOMAIN`] then the CBOR of
    /// the body.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, CborError> {
        let mut out: Vec<u8> = FEED_HEAD_DOMAIN.to_vec();
        let body = spine_cbor::to_vec(self)?;
        out.extend_from_slice(body.as_slice());
        Ok(out)
    }
}

/// The digest a head carries for a set of revocation ids: SHA-256 over
/// [`FEED_DIGEST_DOMAIN`] and the ids sorted ascending, concatenated. The
/// order of `ids` does not matter.
pub fn feed_digest(ids: &[RevocationId]) -> [u8; 32] {
    let mut sorted: Vec<RevocationId> = ids.to_vec();
    sorted.sort();
    let mut hasher = Sha256::new();
    hasher.update(FEED_DIGEST_DOMAIN);
    for id in &sorted {
        hasher.update(id.as_slice());
    }
    hasher.finalize().into()
}

fn ids_of(revocations: &[Revocation]) -> Result<Vec<RevocationId>, CborError> {
    let mut ids: Vec<RevocationId> = Vec::with_capacity(revocations.len());
    for revocation in revocations {
        ids.push(revocation.id()?);
    }
    Ok(ids)
}

/// A signed feed head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedHead {
    /// The signed content.
    pub body: FeedHeadBody,
    /// Ed25519 signature by `body.publisher`.
    #[serde(rename = "sig", with = "crate::bytes::fixed")]
    pub signature: [u8; 64],
}

impl FeedHead {
    /// Signs `body` as it is. The signer should be `body.publisher`.
    pub fn sign(signer: &SigningKey, body: FeedHeadBody) -> Result<FeedHead, CborError> {
        let message = body.signing_bytes()?;
        let signature: Signature = signer.sign(message.as_slice());
        Ok(FeedHead {
            body,
            signature: signature.to_bytes(),
        })
    }

    /// Issues the head for `revocations`: the count and the digest are
    /// computed from them. `sequence` and `as_of` are the caller's.
    pub fn issue(
        signer: &SigningKey,
        sequence: u64,
        as_of: u64,
        revocations: &[Revocation],
    ) -> Result<FeedHead, CborError> {
        let ids = ids_of(revocations)?;
        let body = FeedHeadBody {
            version: FEED_HEAD_VERSION,
            publisher: signer.verifying_key().to_bytes(),
            sequence,
            as_of,
            count: ids.len() as u64,
            digest: feed_digest(ids.as_slice()),
        };
        FeedHead::sign(signer, body)
    }

    /// The canonical encoding of the whole head.
    pub fn encode(&self) -> Result<Vec<u8>, CborError> {
        spine_cbor::to_vec(self)
    }

    /// Decodes a head: exactly the canonical encoding, within
    /// [`MAX_FEED_HEAD_BYTES`].
    pub fn decode(bytes: &[u8]) -> Result<FeedHead, DecodeError> {
        if bytes.len() > MAX_FEED_HEAD_BYTES {
            return Err(DecodeError::TooLarge(MAX_FEED_HEAD_BYTES));
        }
        let head: FeedHead = spine_cbor::from_slice(bytes).map_err(DecodeError::Cbor)?;
        let again = spine_cbor::to_vec(&head).map_err(DecodeError::Cbor)?;
        if again.as_slice() != bytes {
            return Err(DecodeError::NonCanonical);
        }
        Ok(head)
    }

    /// Whether the signature is a valid strict Ed25519 signature by
    /// `publisher`.
    pub fn verify_signature(&self) -> bool {
        let key = match VerifyingKey::from_bytes(&self.body.publisher) {
            Ok(key) => key,
            Err(_) => return false,
        };
        let message = match self.body.signing_bytes() {
            Ok(message) => message,
            Err(_) => return false,
        };
        let signature = Signature::from_bytes(&self.signature);
        key.verify_strict(message.as_slice(), &signature).is_ok()
    }

    /// Checks the version, that the publisher is `expected_publisher`, and
    /// the signature.
    pub fn verify(&self, expected_publisher: &PublicKey) -> Result<(), FeedError> {
        if self.body.version != FEED_HEAD_VERSION {
            return Err(FeedError::UnknownVersion);
        }
        if self.body.publisher != *expected_publisher {
            return Err(FeedError::WrongPublisher);
        }
        if !self.verify_signature() {
            return Err(FeedError::BadHeadSignature);
        }
        Ok(())
    }

    /// Checks that `revocations` is exactly the set this head describes: the
    /// count and the digest over the sorted ids. Duplicates are refused.
    pub fn check_set(&self, revocations: &[Revocation]) -> Result<(), FeedError> {
        if revocations.len() > MAX_FEED_REVOCATIONS {
            return Err(FeedError::TooMany);
        }
        let mut ids = ids_of(revocations).map_err(FeedError::Cbor)?;
        ids.sort();
        if ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(FeedError::DuplicateRevocation);
        }
        if self.body.count != ids.len() as u64 {
            return Err(FeedError::CountMismatch);
        }
        if self.body.digest != feed_digest(ids.as_slice()) {
            return Err(FeedError::DigestMismatch);
        }
        Ok(())
    }
}

/// What the caller supplies to accept a head: whom it trusts, the highest
/// sequence it has already accepted, and its clock.
#[derive(Debug, Clone, Copy)]
pub struct FeedCheck {
    /// The only key whose heads are accepted.
    pub publisher: PublicKey,
    /// The sequence of the last head the caller accepted from this publisher.
    /// `None` on first contact: then no rollback can be detected, and only the
    /// staleness bounds limit how old the data may be.
    pub last_seen_sequence: Option<u64>,
    /// The caller's clock, Unix seconds.
    pub now: u64,
}

/// A verified set of revocations with the head that vouches for it. It
/// answers step 8 of verification.
///
/// A feed is immutable: it can only be built by [`RevocationFeed::load`],
/// which checks everything, so every feed that has data is one whose set
/// matched a head signed by the trusted publisher. A feed built with
/// [`RevocationFeed::default`] has no data and so, with the default
/// [`crate::RevocationBounds`], refuses everything. To take in a new
/// revocation, load again with the publisher's next head.
///
/// Freshness is the head's `as_of`; it is compared with the staleness bound
/// of the invocation's effect class by [`crate::verify`].
#[derive(Debug, Clone, Default)]
pub struct RevocationFeed {
    entries: BTreeMap<EntryKey, Revocation>,
    head: Option<FeedHeadBody>,
}

impl RevocationFeed {
    /// Builds a feed from `revocations` and `head`.
    ///
    /// Refused unless: the head has a known version, is signed by
    /// `check.publisher`, has a sequence above `check.last_seen_sequence`, is
    /// not dated after `check.now`; the set is within
    /// [`MAX_FEED_REVOCATIONS`], has no duplicate, and matches the head's count
    /// and digest; and every revocation has a known version, a valid
    /// signature and a `revoked_at` not after the head's `as_of`.
    pub fn load(
        revocations: Vec<Revocation>,
        head: &FeedHead,
        check: &FeedCheck,
    ) -> Result<RevocationFeed, FeedError> {
        head.verify(&check.publisher)?;
        if let Some(last) = check.last_seen_sequence {
            if head.body.sequence <= last {
                return Err(FeedError::SequenceRollback);
            }
        }
        if head.body.as_of > check.now {
            return Err(FeedError::FutureHead);
        }
        head.check_set(revocations.as_slice())?;
        let mut entries: BTreeMap<EntryKey, Revocation> = BTreeMap::new();
        for revocation in revocations {
            revocation.verify()?;
            if revocation.body.revoked_at > head.body.as_of {
                return Err(FeedError::RevokedAfterHead);
            }
            let id = revocation.id().map_err(FeedError::Cbor)?;
            entries.insert((revocation.body.grant, id), revocation);
        }
        Ok(RevocationFeed {
            entries,
            head: Some(head.body.clone()),
        })
    }

    /// The verified head, or `None` for a feed with no data.
    pub fn head(&self) -> Option<&FeedHeadBody> {
        self.head.as_ref()
    }

    /// The sequence of the verified head. A caller keeps this as its next
    /// `last_seen_sequence`.
    pub fn sequence(&self) -> Option<u64> {
        self.head.as_ref().map(|head| head.sequence)
    }

    /// Number of revocations held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no revocation is held.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn records_for<'a>(&'a self, id: &GrantId) -> impl Iterator<Item = &'a Revocation> + 'a {
        let low: EntryKey = (*id, [0x00u8; 32]);
        let high: EntryKey = (*id, [0xffu8; 32]);
        self.entries.range(low..=high).map(|(_, record)| record)
    }
}

impl RevocationView for RevocationFeed {
    /// Whether any verified record names this grant, without asking who
    /// signed it. This errs toward refusal; [`crate::verify`] uses
    /// [`RevocationView::chain_revoked`], which does ask.
    fn is_revoked(&self, id: &GrantId) -> bool {
        self.records_for(id).next().is_some()
    }

    fn chain_revoked(&self, chain: &Chain, ids: &[GrantId]) -> bool {
        if ids.len() != chain.len() {
            return true;
        }
        for (index, id) in ids.iter().enumerate() {
            if self
                .records_for(id)
                .any(|record| record.is_authorized_at(chain, index))
            {
                return true;
            }
        }
        false
    }

    fn data_age_secs(&self, now: u64) -> Option<u64> {
        self.head
            .as_ref()
            .map(|head| now.saturating_sub(head.as_of))
    }
}
