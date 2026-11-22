//! Key succession: identity that survives a key change.
//!
//! A `did:` name is its inception key, forever. What changes is which key is
//! allowed to sign the name's records. That is decided by a *succession log*, an
//! ordered list of signed [`Succession`] entries that every verifier replays
//! from the inception key. A name with an empty log behaves exactly as a name
//! always has: the inception key signs its records.
//!
//! The scheme is pre-rotation, in the spirit of KERI but minimal and with its
//! own formats. Each `rotate` entry reveals the key that becomes current and
//! commits (SHA-256 under a domain string) to the key that will follow it. An
//! entry is signed by the key current before it, and from the second entry on
//! its revealed key must match the previous entry's commitment, so stealing the
//! current signing key alone cannot produce a valid next entry. A `retire`
//! entry ends the name for good: no key is current afterwards, which is the
//! permanence a tombstone lacks.
//!
//! The first entry has nothing to match: no commitment exists at inception,
//! because the name is only a key. It is authorised by the inception key alone.
//! See `docs/spec/02-names.md` section 10 for the threat analysis.
//!
//! Nothing here is wired into resolution. This module defines the objects, their
//! verification, equivocation detection, and the pure function
//! [`record_authorised`] that says whether a record is signed by the key a log
//! allows.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::record::NameRecord;
use crate::NameError;

/// Domain-separation prefix of the signed bytes of an entry.
pub const SUCCESSION_SIGNING_DOMAIN: &[u8] = b"spine-name-succession-v1\0";

/// Domain-separation prefix of an entry hash.
pub const SUCCESSION_ENTRY_DOMAIN: &[u8] = b"spine-name-succession-entry-v1\0";

/// Domain-separation prefix of a next-key commitment.
pub const SUCCESSION_COMMITMENT_DOMAIN: &[u8] = b"spine-name-succession-next-v1\0";

/// Most entries a log may hold. A longer log is refused whole.
pub const MAX_SUCCESSION_ENTRIES: usize = 64;

/// The reserved `meta` key of a record that states the log position its signer
/// holds. Its value is `<index>:<head hash as 64 lowercase hex digits>`.
pub const SUCCESSION_META_KEY: &str = "spine.succession";

/// The commitment to a next key: SHA-256 over
/// [`SUCCESSION_COMMITMENT_DOMAIN`] and the 32-byte public key.
pub fn key_commitment(key: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(SUCCESSION_COMMITMENT_DOMAIN);
    hasher.update(key);
    hasher.finalize().into()
}

/// What an entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SuccessionKind {
    /// The revealed key becomes current; the entry commits to its successor.
    Rotate,
    /// The name is permanently revoked. No key is current afterwards.
    Retire,
}

impl SuccessionKind {
    fn tag(self) -> u8 {
        match self {
            SuccessionKind::Rotate => 0,
            SuccessionKind::Retire => 1,
        }
    }
}

/// One signed step of a succession log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Succession {
    /// The name's inception key: the key in its `did:` authority.
    #[serde(with = "key32")]
    pub inception_key: [u8; 32],
    /// Position in the log, starting at 1.
    pub index: u64,
    /// Hash of the previous entry; `None` exactly for index 1.
    #[serde(with = "opt32")]
    pub prev: Option<[u8; 32]>,
    /// Rotate or retire.
    pub kind: SuccessionKind,
    /// The revealed key. For a rotate it becomes the current key. For a retire
    /// at index 1 it is the inception key; for a retire at a later index it is
    /// the key committed to by the previous entry, revealed to show that more
    /// than the current signing key authorised the retirement.
    #[serde(with = "key32")]
    pub key: [u8; 32],
    /// Commitment to the key after `key`: present for a rotate, absent for a
    /// retire.
    #[serde(with = "opt32")]
    pub commitment: Option<[u8; 32]>,
    /// When the entry says it takes effect, Unix seconds. Signed and required
    /// not to decrease along a log; authorisation does not depend on it.
    pub effective_at: u64,
    /// Ed25519 signature over [`Succession::signing_bytes`] by the key current
    /// before this entry (the inception key for index 1).
    #[serde(with = "sig_bytes")]
    pub signature: Vec<u8>,
}

fn put_opt(out: &mut Vec<u8>, value: Option<&[u8; 32]>) {
    match value {
        Some(v) => {
            out.push(1);
            out.extend_from_slice(v);
        }
        None => out.push(0),
    }
}

impl Succession {
    /// The bytes the signature covers. Every field but the signature, in a
    /// fixed layout with no variable-length parts except through the one-byte
    /// presence flags:
    ///
    /// ```text
    /// "spine-name-succession-v1\0"          25 bytes, domain separation
    /// inception_key                          32 bytes
    /// u64 index                              big-endian
    /// u8 has_prev (0|1), then 32 bytes iff 1
    /// u8 kind (0 rotate, 1 retire)
    /// key                                    32 bytes
    /// u8 has_commitment (0|1), then 32 bytes iff 1
    /// u64 effective_at                       big-endian
    /// ```
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(160);
        out.extend_from_slice(SUCCESSION_SIGNING_DOMAIN);
        out.extend_from_slice(&self.inception_key);
        out.extend_from_slice(&self.index.to_be_bytes());
        put_opt(&mut out, self.prev.as_ref());
        out.push(self.kind.tag());
        out.extend_from_slice(&self.key);
        put_opt(&mut out, self.commitment.as_ref());
        out.extend_from_slice(&self.effective_at.to_be_bytes());
        out
    }

    /// The entry hash: SHA-256 over [`SUCCESSION_ENTRY_DOMAIN`], the signed
    /// bytes and the signature. The next entry's `prev` carries it.
    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(SUCCESSION_ENTRY_DOMAIN);
        hasher.update(self.signing_bytes());
        hasher.update(self.signature.as_slice());
        hasher.finalize().into()
    }

    /// Sign with `key`. This does not check that `key` is authorised to sign
    /// the entry or that the entry is well formed: those are the business of
    /// [`State::apply`], so that invalid entries can be built for tests.
    pub fn sign(&mut self, key: &SigningKey) {
        let sig: Signature = key.sign(&self.signing_bytes());
        self.signature = sig.to_bytes().to_vec();
    }
}

/// Which key may sign a name's records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Current {
    /// This key signs the name's records.
    Key([u8; 32]),
    /// The name is retired: no key does, ever.
    Retired,
}

/// Why a log, entry, equivocation proof or head claim is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SuccessionError {
    #[error("succession log has {len} entries, more than the allowed maximum")]
    TooManyEntries { len: usize },

    #[error("entry after the name was retired")]
    AfterRetire,

    #[error("entry index {got} where {expected} was expected")]
    IndexGap { expected: u64, got: u64 },

    #[error("entry names a different inception key")]
    InceptionMismatch,

    #[error("entry does not carry the hash of the previous entry")]
    PrevMismatch,

    #[error("malformed entry: {0}")]
    Malformed(&'static str),

    #[error("entry takes effect before the previous one")]
    TimeRegression,

    #[error("entry signature does not verify under the current key")]
    BadSignature,

    #[error("revealed key does not match the previous commitment")]
    CommitmentMismatch,

    #[error("rotation to a key the name has already used")]
    KeyReuse,

    #[error("the two entries are the same entry, so there is no equivocation")]
    NotEquivocation,

    #[error("malformed succession head claim")]
    MalformedClaim,
}

impl SuccessionError {
    /// A stable lower-case reason, the spelling used in the test vectors.
    pub fn reason(&self) -> &'static str {
        match self {
            SuccessionError::TooManyEntries { .. } => "too_many_entries",
            SuccessionError::AfterRetire => "after_retire",
            SuccessionError::IndexGap { .. } => "index_gap",
            SuccessionError::InceptionMismatch => "inception_mismatch",
            SuccessionError::PrevMismatch => "prev_mismatch",
            SuccessionError::Malformed(_) => "malformed",
            SuccessionError::TimeRegression => "time_regression",
            SuccessionError::BadSignature => "bad_signature",
            SuccessionError::CommitmentMismatch => "commitment_mismatch",
            SuccessionError::KeyReuse => "key_reuse",
            SuccessionError::NotEquivocation => "not_equivocation",
            SuccessionError::MalformedClaim => "malformed_claim",
        }
    }
}

/// The result of replaying a log: who may sign, and where the log stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    inception_key: [u8; 32],
    index: u64,
    head: Option<[u8; 32]>,
    current: Current,
    commitment: Option<[u8; 32]>,
    effective_at: u64,
    previous_keys: Vec<[u8; 32]>,
}

impl State {
    /// The state of a name whose log is empty: the inception key is current.
    pub fn inception(inception_key: [u8; 32]) -> Self {
        Self {
            inception_key,
            index: 0,
            head: None,
            current: Current::Key(inception_key),
            commitment: None,
            effective_at: 0,
            previous_keys: Vec::new(),
        }
    }

    /// The name's inception key.
    pub fn inception_key(&self) -> &[u8; 32] {
        &self.inception_key
    }

    /// Number of entries applied; 0 for an empty log.
    pub fn index(&self) -> u64 {
        self.index
    }

    /// Hash of the last entry; `None` for an empty log.
    pub fn head(&self) -> Option<&[u8; 32]> {
        self.head.as_ref()
    }

    /// The key that may sign records, or retired.
    pub fn current(&self) -> Current {
        self.current
    }

    /// The commitment the next entry's revealed key must match; `None` for an
    /// empty log (no commitment exists yet) and after a retire.
    pub fn commitment(&self) -> Option<&[u8; 32]> {
        self.commitment.as_ref()
    }

    /// Keys that were current once and no longer are, oldest first.
    pub fn superseded_keys(&self) -> &[[u8; 32]] {
        &self.previous_keys
    }

    /// Whether the name is permanently retired.
    pub fn is_retired(&self) -> bool {
        self.current == Current::Retired
    }

    /// The head claim a record signed at this position should carry, or `None`
    /// for an empty log.
    pub fn head_claim(&self) -> Option<HeadClaim> {
        self.head.map(|head| HeadClaim {
            index: self.index,
            head,
        })
    }

    /// Apply one entry. Checks, in this order: not after a retire, index is
    /// the next one, inception key, previous hash, shape (a rotate has a
    /// commitment, a retire has none, a retire at index 1 reveals the
    /// inception key), effective time not decreasing, signature (strict
    /// Ed25519, under the current key), commitment match (when one is held),
    /// and, for a rotate, that the new key is not one the name has used.
    pub fn apply(&self, entry: &Succession) -> Result<State, SuccessionError> {
        let Current::Key(signer) = self.current else {
            return Err(SuccessionError::AfterRetire);
        };
        let expected = self.index + 1;
        if entry.index != expected {
            return Err(SuccessionError::IndexGap {
                expected,
                got: entry.index,
            });
        }
        if entry.inception_key != self.inception_key {
            return Err(SuccessionError::InceptionMismatch);
        }
        if entry.prev != self.head {
            return Err(SuccessionError::PrevMismatch);
        }
        match (entry.kind, entry.commitment) {
            (SuccessionKind::Rotate, None) => {
                return Err(SuccessionError::Malformed("a rotate carries a commitment"))
            }
            (SuccessionKind::Retire, Some(_)) => {
                return Err(SuccessionError::Malformed("a retire carries no commitment"))
            }
            _ => {}
        }
        if entry.index == 1
            && entry.kind == SuccessionKind::Retire
            && entry.key != self.inception_key
        {
            return Err(SuccessionError::Malformed(
                "a retire at index 1 reveals the inception key",
            ));
        }
        if entry.effective_at < self.effective_at {
            return Err(SuccessionError::TimeRegression);
        }
        verify_entry_signature(entry, &signer)?;
        if let Some(committed) = self.commitment {
            if key_commitment(&entry.key) != committed {
                return Err(SuccessionError::CommitmentMismatch);
            }
        }
        if entry.kind == SuccessionKind::Rotate
            && (entry.key == signer || self.previous_keys.contains(&entry.key))
        {
            return Err(SuccessionError::KeyReuse);
        }

        let mut previous_keys = self.previous_keys.clone();
        previous_keys.push(signer);
        let (current, commitment) = match entry.kind {
            SuccessionKind::Rotate => (Current::Key(entry.key), entry.commitment),
            SuccessionKind::Retire => (Current::Retired, None),
        };
        Ok(State {
            inception_key: self.inception_key,
            index: entry.index,
            head: Some(entry.hash()),
            current,
            commitment,
            effective_at: entry.effective_at,
            previous_keys,
        })
    }
}

fn verify_entry_signature(entry: &Succession, signer: &[u8; 32]) -> Result<(), SuccessionError> {
    if entry.signature.len() != 64 {
        return Err(SuccessionError::BadSignature);
    }
    let vk = VerifyingKey::from_bytes(signer).map_err(|_| SuccessionError::BadSignature)?;
    let mut sig = [0u8; 64];
    sig.copy_from_slice(&entry.signature);
    vk.verify_strict(&entry.signing_bytes(), &Signature::from_bytes(&sig))
        .map_err(|_| SuccessionError::BadSignature)
}

/// An ordered list of entries. Order is the only structure: a fork cannot be
/// represented in one log.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SuccessionLog {
    pub entries: Vec<Succession>,
}

impl SuccessionLog {
    /// Replay the log from `inception_key`. An empty log yields
    /// [`State::inception`]. More than [`MAX_SUCCESSION_ENTRIES`] entries is
    /// refused before any entry is looked at.
    pub fn verify(&self, inception_key: &[u8; 32]) -> Result<State, SuccessionError> {
        if self.entries.len() > MAX_SUCCESSION_ENTRIES {
            return Err(SuccessionError::TooManyEntries {
                len: self.entries.len(),
            });
        }
        self.entries
            .iter()
            .try_fold(State::inception(*inception_key), |state, e| state.apply(e))
    }
}

/// Proof that the key current at some position signed two different entries
/// there. It carries the shared prefix so that anyone can check it from the
/// inception key alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Equivocation {
    /// The entries both logs share, before the point where they differ.
    pub prefix: SuccessionLog,
    /// One entry at the divergence index.
    pub first: Succession,
    /// A different entry at the same index.
    pub second: Succession,
}

impl Equivocation {
    /// The index at which the two entries differ.
    pub fn index(&self) -> u64 {
        self.first.index
    }

    /// Check the proof: the prefix is a valid log, the two entries differ, and
    /// each is a valid next entry after the prefix.
    pub fn verify(&self, inception_key: &[u8; 32]) -> Result<(), SuccessionError> {
        let state = self.prefix.verify(inception_key)?;
        if self.first == self.second {
            return Err(SuccessionError::NotEquivocation);
        }
        state.apply(&self.first)?;
        state.apply(&self.second)?;
        Ok(())
    }
}

/// Compare two logs of one name. Both must verify (otherwise the error of the
/// first that does not). If one is a prefix of the other, or they are equal,
/// the result is `None`: a verifier simply keeps the longer. Otherwise they
/// diverge at the first index where the entries differ; both are valid there
/// and share the same prefix, so the same key signed two entries at one index:
/// equivocation.
pub fn detect_equivocation(
    inception_key: &[u8; 32],
    a: &SuccessionLog,
    b: &SuccessionLog,
) -> Result<Option<Equivocation>, SuccessionError> {
    a.verify(inception_key)?;
    b.verify(inception_key)?;
    for (i, (x, y)) in a.entries.iter().zip(b.entries.iter()).enumerate() {
        if x != y {
            return Ok(Some(Equivocation {
                prefix: SuccessionLog {
                    entries: a.entries[..i].to_vec(),
                },
                first: x.clone(),
                second: y.clone(),
            }));
        }
    }
    Ok(None)
}

/// The log position a record's signer says it holds: the number of entries and
/// the hash of the last. Carried in the record's `meta` under
/// [`SUCCESSION_META_KEY`]; index 0 has no claim (the key is absent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadClaim {
    /// Entries in the log, 1 to [`MAX_SUCCESSION_ENTRIES`].
    pub index: u64,
    /// Hash of that entry.
    pub head: [u8; 32],
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

fn from_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(s.as_bytes().chunks(2)) {
        *slot = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(out)
}

impl HeadClaim {
    /// The `meta` value: `<decimal index>:<64 lowercase hex digits>`.
    pub fn to_meta_value(&self) -> String {
        let hex: String = self.head.iter().map(|b| format!("{b:02x}")).collect();
        format!("{}:{hex}", self.index)
    }

    /// Parse a `meta` value. The index is canonical decimal (no sign, no
    /// leading zero) from 1 to [`MAX_SUCCESSION_ENTRIES`]; the hash is exactly
    /// 64 lowercase hex digits.
    pub fn parse(value: &str) -> Result<Self, SuccessionError> {
        let bad = || SuccessionError::MalformedClaim;
        let (idx, head) = value.split_once(':').ok_or_else(bad)?;
        if idx.is_empty() || !idx.bytes().all(|b| b.is_ascii_digit()) || idx.starts_with('0') {
            return Err(bad());
        }
        let index: u64 = idx.parse().map_err(|_| bad())?;
        if index > MAX_SUCCESSION_ENTRIES as u64 {
            return Err(bad());
        }
        let head = from_hex32(head).ok_or_else(bad)?;
        Ok(Self { index, head })
    }
}

/// The head claim a record carries, if any. A present but malformed value is an
/// error, not an absence.
pub fn record_head_claim(record: &NameRecord) -> Result<Option<HeadClaim>, SuccessionError> {
    record
        .meta
        .get(SUCCESSION_META_KEY)
        .map(|v| HeadClaim::parse(v))
        .transpose()
}

/// Why a record is not authorised.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The record's name is not a `did:` name with the log's inception key.
    WrongName,
    /// The record's `spine.succession` value is malformed.
    MalformedClaim,
    /// The record claims a position older than the verifier holds.
    StaleClaim { claimed: u64, held: u64 },
    /// The record claims the verifier's position with a different head: the
    /// two disagree about what entry that is.
    HeadConflict,
    /// The name is retired; no key may sign for it.
    Retired,
    /// Signed by a key that a held entry has superseded.
    SupersededKey,
    /// The signature does not verify under the current key, nor under any
    /// superseded one.
    BadSignature,
    /// The signature verifies but the record breaks a record rule (lifetime or
    /// tombstone shape, or the name is not signable).
    Record(NameError),
}

impl Refusal {
    /// A stable lower-case reason, the spelling used in the test vectors.
    pub fn reason(&self) -> &'static str {
        match self {
            Refusal::WrongName => "wrong_name",
            Refusal::MalformedClaim => "malformed_claim",
            Refusal::StaleClaim { .. } => "stale_claim",
            Refusal::HeadConflict => "head_conflict",
            Refusal::Retired => "retired",
            Refusal::SupersededKey => "superseded_key",
            Refusal::BadSignature => "bad_signature",
            Refusal::Record(_) => "record_invalid",
        }
    }
}

/// The verdict of [`record_authorised`].
#[derive(Debug, PartialEq, Eq)]
pub enum RecordAuthority {
    /// The record is signed by the key the log allows.
    Authorised,
    /// The record claims a log position beyond what the verifier holds. It must
    /// fetch the log (up to at least `claimed.index`) before deciding.
    NeedLog { claimed: HeadClaim, held: u64 },
    /// The record is not authorised.
    Refused(Refusal),
}

/// Whether `record` is signed by the key that the verifier's best log for the
/// name allows. Pure: no clock, no store.
///
/// `log` is the [`State`] of the verifier's best verified log for this name, or
/// `None` if it holds none, which is the same as an empty log: the key in the
/// record's name is current. The rules, in order:
///
/// 1. The record's name is a `did:` name whose key is the state's inception
///    key, else [`Refusal::WrongName`].
/// 2. A `spine.succession` claim, if present, is well formed
///    ([`Refusal::MalformedClaim`]). A claim of an index above the state's is
///    [`RecordAuthority::NeedLog`] (decided before the signature, which cannot
///    be judged without the log); below it, [`Refusal::StaleClaim`]; at it, the
///    heads must be equal ([`Refusal::HeadConflict`]).
/// 3. A retired name authorises nothing ([`Refusal::Retired`]).
/// 4. The signature verifies under the current key
///    ([`NameRecord::verify_with`]). If not, and it verifies under a superseded
///    key, [`Refusal::SupersededKey`]; otherwise [`Refusal::BadSignature`].
///
/// A record signed by a superseded key is refused whatever its remaining
/// lifetime: that is the purpose of rotation. A record without a claim is
/// accepted if the signature is right, so a verifier that has not yet seen the
/// newest entry still accepts records of the key it believes current; that
/// exposure ends with the record's lifetime, at most
/// [`crate::MAX_RECORD_TTL_SECS`].
pub fn record_authorised(record: &NameRecord, log: Option<&State>) -> RecordAuthority {
    let Some(name_key) = record.name.public_key() else {
        return RecordAuthority::Refused(Refusal::Record(NameError::NotSignable(
            record.name.to_string(),
        )));
    };
    let state = match log {
        Some(s) => s.clone(),
        None => State::inception(*name_key),
    };
    if name_key != state.inception_key() {
        return RecordAuthority::Refused(Refusal::WrongName);
    }

    match record_head_claim(record) {
        Err(_) => return RecordAuthority::Refused(Refusal::MalformedClaim),
        Ok(None) => {}
        Ok(Some(claim)) => {
            if claim.index > state.index() {
                return RecordAuthority::NeedLog {
                    claimed: claim,
                    held: state.index(),
                };
            }
            if claim.index < state.index() {
                return RecordAuthority::Refused(Refusal::StaleClaim {
                    claimed: claim.index,
                    held: state.index(),
                });
            }
            if state.head() != Some(&claim.head) {
                return RecordAuthority::Refused(Refusal::HeadConflict);
            }
        }
    }

    let Current::Key(current) = state.current() else {
        return RecordAuthority::Refused(Refusal::Retired);
    };
    match record.verify_with(&current) {
        Ok(()) => RecordAuthority::Authorised,
        Err(NameError::BadSignature) => {
            if state
                .superseded_keys()
                .iter()
                .any(|old| record.verify_with(old).is_ok())
            {
                RecordAuthority::Refused(Refusal::SupersededKey)
            } else {
                RecordAuthority::Refused(Refusal::BadSignature)
            }
        }
        Err(other) => RecordAuthority::Refused(Refusal::Record(other)),
    }
}

/// Serde helper: a 32-byte value travels as base32, like keys elsewhere.
mod key32 {
    use crate::base32;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base32::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let raw = String::deserialize(d)?;
        base32::decode_key(&raw)
            .ok_or_else(|| serde::de::Error::custom("expected 32-byte base32 value"))
    }
}

/// Serde helper: an optional 32-byte value, `null` when absent.
mod opt32 {
    use crate::base32;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Option<[u8; 32]>, s: S) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(b) => s.serialize_some(&base32::encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[u8; 32]>, D::Error> {
        let raw = Option::<String>::deserialize(d)?;
        raw.map(|r| {
            base32::decode_key(&r)
                .ok_or_else(|| serde::de::Error::custom("expected 32-byte base32 value"))
        })
        .transpose()
    }
}

/// Serde helper: signatures as base32, like record signatures.
mod sig_bytes {
    use crate::base32;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base32::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let raw = String::deserialize(d)?;
        base32::decode(&raw).ok_or_else(|| serde::de::Error::custom("invalid base32 signature"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uri::SpineUri;
    use serde_json::Value;

    fn sk(n: u8) -> SigningKey {
        SigningKey::from_bytes(&[n; 32])
    }

    fn pk(k: &SigningKey) -> [u8; 32] {
        k.verifying_key().to_bytes()
    }

    fn keys(n: usize) -> Vec<SigningKey> {
        (1..=n as u8).map(sk).collect()
    }

    fn rotate(
        log: &SuccessionLog,
        signer: &SigningKey,
        new: &SigningKey,
        next: &SigningKey,
        at: u64,
    ) -> Succession {
        // The inception key is the first key of `keys`, i.e. sk(1).
        let mut e = Succession {
            inception_key: pk(&sk(1)),
            index: log.entries.len() as u64 + 1,
            prev: log.entries.last().map(Succession::hash),
            kind: SuccessionKind::Rotate,
            key: pk(new),
            commitment: Some(key_commitment(&pk(next))),
            effective_at: at,
            signature: Vec::new(),
        };
        e.sign(signer);
        e
    }

    fn retire(
        log: &SuccessionLog,
        signer: &SigningKey,
        revealed: &SigningKey,
        at: u64,
    ) -> Succession {
        let mut e = Succession {
            inception_key: pk(&sk(1)),
            index: log.entries.len() as u64 + 1,
            prev: log.entries.last().map(Succession::hash),
            kind: SuccessionKind::Retire,
            key: pk(revealed),
            commitment: None,
            effective_at: at,
            signature: Vec::new(),
        };
        e.sign(signer);
        e
    }

    /// A valid log of `n` rotations: inception sk(1), then sk(2), sk(3), ...
    fn chain(n: usize) -> SuccessionLog {
        let k = keys(n + 2);
        let mut log = SuccessionLog::default();
        for i in 0..n {
            let e = rotate(&log, &k[i], &k[i + 1], &k[i + 2], 100 + i as u64);
            log.entries.push(e);
        }
        log
    }

    fn inception() -> [u8; 32] {
        pk(&sk(1))
    }

    #[test]
    fn empty_log_leaves_the_inception_key_current() {
        let st = SuccessionLog::default().verify(&inception()).unwrap();
        assert_eq!(st.index(), 0);
        assert_eq!(st.head(), None);
        assert_eq!(st.current(), Current::Key(inception()));
        assert_eq!(st.commitment(), None);
        assert!(!st.is_retired());
    }

    #[test]
    fn rotations_move_the_current_key() {
        let log = chain(3);
        let st = log.verify(&inception()).unwrap();
        assert_eq!(st.index(), 3);
        assert_eq!(st.current(), Current::Key(pk(&sk(4))));
        assert_eq!(st.head(), Some(&log.entries[2].hash()));
        assert_eq!(st.commitment(), Some(&key_commitment(&pk(&sk(5)))));
        assert_eq!(st.superseded_keys(), &[pk(&sk(1)), pk(&sk(2)), pk(&sk(3))]);
    }

    #[test]
    fn rotate_then_retire_ends_the_name() {
        let mut log = chain(1);
        let k = keys(3);
        let r = retire(&log, &k[1], &k[2], 200);
        log.entries.push(r);
        let st = log.verify(&inception()).unwrap();
        assert!(st.is_retired());
        assert_eq!(st.index(), 2);
        assert_eq!(st.commitment(), None);
    }

    #[test]
    fn retire_at_once_reveals_the_inception_key() {
        let k = keys(2);
        let mut log = SuccessionLog::default();
        let r = retire(&log, &k[0], &k[0], 50);
        log.entries.push(r);
        assert!(log.verify(&inception()).unwrap().is_retired());

        let mut bad = SuccessionLog::default();
        let r = retire(&bad, &k[0], &k[1], 50);
        bad.entries.push(r);
        assert!(matches!(
            bad.verify(&inception()),
            Err(SuccessionError::Malformed(_))
        ));
    }

    #[test]
    fn wrong_signer_is_refused() {
        let k = keys(4);
        // Entry 1 signed by the next key rather than the inception key.
        let mut log = SuccessionLog::default();
        let e = rotate(&log, &k[1], &k[1], &k[2], 100);
        log.entries.push(e);
        assert_eq!(log.verify(&inception()), Err(SuccessionError::BadSignature));
        // Entry 2 signed by the inception key (superseded) instead of key 2.
        let mut log = chain(1);
        let e = rotate(&log, &k[0], &k[1], &k[2], 101);
        log.entries.push(e);
        assert_eq!(log.verify(&inception()), Err(SuccessionError::BadSignature));
    }

    #[test]
    fn current_key_alone_cannot_choose_the_next_key() {
        // The holder of key 2 (current after entry 1) tries to rotate to a key
        // of its own choosing rather than the committed key 3.
        let k = keys(6);
        let mut log = chain(1);
        let e = rotate(&log, &k[1], &k[5], &k[4], 101);
        log.entries.push(e);
        assert_eq!(
            log.verify(&inception()),
            Err(SuccessionError::CommitmentMismatch)
        );
    }

    #[test]
    fn current_key_alone_cannot_retire() {
        // A retire at index 2 must reveal the committed key; the attacker
        // holding only the current key reveals its own.
        let k = keys(6);
        let mut log = chain(1);
        let e = retire(&log, &k[1], &k[5], 101);
        log.entries.push(e);
        assert_eq!(
            log.verify(&inception()),
            Err(SuccessionError::CommitmentMismatch)
        );
    }

    #[test]
    fn index_gap_is_refused() {
        let mut log = chain(2);
        log.entries[1].index = 3;
        assert_eq!(
            log.verify(&inception()),
            Err(SuccessionError::IndexGap {
                expected: 2,
                got: 3
            })
        );
        let mut log = chain(1);
        log.entries[0].index = 2;
        assert_eq!(
            log.verify(&inception()),
            Err(SuccessionError::IndexGap {
                expected: 1,
                got: 2
            })
        );
    }

    #[test]
    fn prev_hash_mismatch_is_refused() {
        let mut log = chain(2);
        log.entries[1].prev = Some([9u8; 32]);
        assert_eq!(log.verify(&inception()), Err(SuccessionError::PrevMismatch));
        let mut log = chain(1);
        log.entries[0].prev = Some([0u8; 32]);
        assert_eq!(log.verify(&inception()), Err(SuccessionError::PrevMismatch));
        let mut log = chain(2);
        log.entries[1].prev = None;
        assert_eq!(log.verify(&inception()), Err(SuccessionError::PrevMismatch));
    }

    #[test]
    fn entry_after_retire_is_refused() {
        let k = keys(5);
        let mut log = chain(1);
        let r = retire(&log, &k[1], &k[2], 200);
        log.entries.push(r);
        let e = rotate(&log, &k[2], &k[3], &k[4], 300);
        log.entries.push(e);
        assert_eq!(log.verify(&inception()), Err(SuccessionError::AfterRetire));
    }

    #[test]
    fn log_over_the_bound_is_refused_and_at_the_bound_is_accepted() {
        let at = chain(MAX_SUCCESSION_ENTRIES);
        assert_eq!(
            at.verify(&inception()).unwrap().index(),
            MAX_SUCCESSION_ENTRIES as u64
        );
        let over = chain(MAX_SUCCESSION_ENTRIES + 1);
        assert_eq!(
            over.verify(&inception()),
            Err(SuccessionError::TooManyEntries {
                len: MAX_SUCCESSION_ENTRIES + 1
            })
        );
    }

    #[test]
    fn every_tampered_field_is_refused() {
        let base = chain(3);
        let tamper = |f: &dyn Fn(&mut Succession)| {
            let mut log = base.clone();
            f(&mut log.entries[1]);
            log.verify(&inception())
        };
        assert_eq!(
            tamper(&|e| e.inception_key[0] ^= 1),
            Err(SuccessionError::InceptionMismatch)
        );
        assert!(matches!(
            tamper(&|e| e.index += 1),
            Err(SuccessionError::IndexGap { .. })
        ));
        assert_eq!(
            tamper(&|e| e.prev = Some([1u8; 32])),
            Err(SuccessionError::PrevMismatch)
        );
        assert!(matches!(
            tamper(&|e| e.kind = SuccessionKind::Retire),
            Err(SuccessionError::Malformed(_))
        ));
        assert_eq!(
            tamper(&|e| e.key[0] ^= 1),
            Err(SuccessionError::BadSignature)
        );
        assert_eq!(
            tamper(&|e| e.commitment = Some([2u8; 32])),
            Err(SuccessionError::BadSignature)
        );
        assert_eq!(
            tamper(&|e| e.effective_at += 1),
            Err(SuccessionError::BadSignature)
        );
        assert_eq!(
            tamper(&|e| e.signature[0] ^= 1),
            Err(SuccessionError::BadSignature)
        );
        assert_eq!(
            tamper(&|e| e.signature.truncate(63)),
            Err(SuccessionError::BadSignature)
        );
    }

    #[test]
    fn effective_time_must_not_decrease() {
        let k = keys(4);
        let mut log = chain(1);
        let e = rotate(&log, &k[1], &k[2], &k[3], 99);
        log.entries.push(e);
        assert_eq!(
            log.verify(&inception()),
            Err(SuccessionError::TimeRegression)
        );
    }

    #[test]
    fn rotation_to_a_used_key_is_refused() {
        // Entry 1 commits to the inception key itself; entry 2 rotates to it.
        let k = keys(3);
        let mut log = SuccessionLog::default();
        let e = rotate(&log, &k[0], &k[1], &k[0], 100);
        log.entries.push(e);
        let e = rotate(&log, &k[1], &k[0], &k[2], 101);
        log.entries.push(e);
        assert_eq!(log.verify(&inception()), Err(SuccessionError::KeyReuse));
    }

    #[test]
    fn shape_is_enforced() {
        let k = keys(3);
        let mut log = SuccessionLog::default();
        let mut e = rotate(&log, &k[0], &k[1], &k[2], 100);
        e.commitment = None;
        e.sign(&k[0]);
        log.entries.push(e);
        assert!(matches!(
            log.verify(&inception()),
            Err(SuccessionError::Malformed(_))
        ));

        let mut log = chain(1);
        let mut r = retire(&log, &k[1], &k[2], 200);
        r.commitment = Some([3u8; 32]);
        r.sign(&k[1]);
        log.entries.push(r);
        assert!(matches!(
            log.verify(&inception()),
            Err(SuccessionError::Malformed(_))
        ));
    }

    #[test]
    fn signing_bytes_layout_is_fixed() {
        let log = chain(2);
        let e0 = &log.entries[0];
        let b0 = e0.signing_bytes();
        assert_eq!(&b0[..25], b"spine-name-succession-v1\0");
        assert_eq!(b0.len(), 25 + 32 + 8 + 1 + 1 + 32 + 33 + 8);
        assert_eq!(&b0[25..57], &e0.inception_key);
        assert_eq!(&b0[57..65], &1u64.to_be_bytes());
        assert_eq!(b0[65], 0);
        assert_eq!(b0[66], 0);
        assert_eq!(&b0[67..99], &e0.key);
        assert_eq!(b0[99], 1);
        let e1 = &log.entries[1];
        let b1 = e1.signing_bytes();
        assert_eq!(b1.len(), b0.len() + 32);
        assert_eq!(b1[65], 1);
        assert_eq!(&b1[66..98], &e0.hash());
        assert_ne!(e0.hash(), e1.hash());
    }

    #[test]
    fn entries_round_trip_through_json() {
        let log = chain(2);
        let json = serde_json::to_string(&log).unwrap();
        let back: SuccessionLog = serde_json::from_str(&json).unwrap();
        assert_eq!(back, log);
        assert_eq!(serde_json::to_value(&log).unwrap()[0]["prev"], Value::Null);
    }

    fn equivocating_logs() -> (SuccessionLog, SuccessionLog) {
        let k = keys(5);
        let a = chain(2);
        let mut b = chain(1);
        // Same signer (key 2) signs a different second entry.
        let e = rotate(&b, &k[1], &k[2], &k[3], 777);
        b.entries.push(e);
        (a, b)
    }

    #[test]
    fn equivocation_is_detected_and_provable() {
        let (a, b) = equivocating_logs();
        let proof = detect_equivocation(&inception(), &a, &b)
            .unwrap()
            .expect("a fork at index 2");
        assert_eq!(proof.index(), 2);
        assert_eq!(proof.prefix.entries.len(), 1);
        assert_eq!(proof.verify(&inception()), Ok(()));
        // Order does not matter for detection.
        assert!(detect_equivocation(&inception(), &b, &a).unwrap().is_some());
    }

    #[test]
    fn equivocation_at_index_one() {
        let k = keys(7);
        let a = chain(1);
        let mut b = SuccessionLog::default();
        let e = rotate(&b, &k[0], &k[5], &k[6], 100);
        b.entries.push(e);
        let proof = detect_equivocation(&inception(), &a, &b).unwrap().unwrap();
        assert_eq!(proof.index(), 1);
        assert!(proof.prefix.entries.is_empty());
        assert_eq!(proof.verify(&inception()), Ok(()));
    }

    #[test]
    fn prefix_and_identical_logs_are_not_equivocation() {
        let a = chain(2);
        let b = chain(1);
        assert_eq!(detect_equivocation(&inception(), &a, &b), Ok(None));
        assert_eq!(detect_equivocation(&inception(), &b, &a), Ok(None));
        assert_eq!(detect_equivocation(&inception(), &a, &a), Ok(None));
        let empty = SuccessionLog::default();
        assert_eq!(detect_equivocation(&inception(), &empty, &a), Ok(None));
    }

    #[test]
    fn equivocation_needs_two_valid_logs_and_a_checkable_proof() {
        let (a, mut b) = equivocating_logs();
        b.entries[1].effective_at += 1;
        assert_eq!(
            detect_equivocation(&inception(), &a, &b),
            Err(SuccessionError::BadSignature)
        );

        let (a, b) = equivocating_logs();
        let mut proof = detect_equivocation(&inception(), &a, &b).unwrap().unwrap();
        proof.second = proof.first.clone();
        assert_eq!(
            proof.verify(&inception()),
            Err(SuccessionError::NotEquivocation)
        );
        let mut proof = detect_equivocation(&inception(), &a, &b).unwrap().unwrap();
        proof.second.signature[0] ^= 1;
        assert_eq!(
            proof.verify(&inception()),
            Err(SuccessionError::BadSignature)
        );
        let proof = detect_equivocation(&inception(), &a, &b).unwrap().unwrap();
        assert_eq!(
            proof.verify(&pk(&sk(9))),
            Err(SuccessionError::InceptionMismatch)
        );
    }

    #[test]
    fn head_claims_round_trip_and_reject_noise() {
        let claim = HeadClaim {
            index: 3,
            head: [0xab; 32],
        };
        let text = claim.to_meta_value();
        assert_eq!(text, format!("3:{}", "ab".repeat(32)));
        assert_eq!(HeadClaim::parse(&text), Ok(claim));
        let h = "ab".repeat(32);
        for bad in [
            String::new(),
            "3".to_string(),
            format!("0:{h}"),
            format!("03:{h}"),
            format!("+3:{h}"),
            format!("65:{h}"),
            format!("3:{}", h.to_uppercase()),
            format!("3:{}", &h[..62]),
            format!("3:{h}00"),
            format!(":{h}"),
            format!("3:{}zz", &h[..62]),
            format!("99999999999999999999:{h}"),
        ] {
            assert_eq!(
                HeadClaim::parse(&bad),
                Err(SuccessionError::MalformedClaim),
                "{bad:?}"
            );
        }
    }

    fn name() -> SpineUri {
        SpineUri::did(inception())
    }

    fn record(signer: &SigningKey, claim: Option<HeadClaim>) -> NameRecord {
        let mut r = NameRecord::new(name(), 1, 1_000)
            .unwrap()
            .with_ttl(crate::MAX_RECORD_TTL_SECS);
        if let Some(c) = claim {
            r = r.with_meta(SUCCESSION_META_KEY, c.to_meta_value());
        }
        // `sign` insists on the name's key, so sign the bytes by hand.
        let sig: Signature = signer.sign(&r.signing_bytes());
        r.signature = sig.to_bytes().to_vec();
        r
    }

    #[test]
    fn without_a_log_the_inception_key_signs() {
        let k = keys(2);
        assert_eq!(
            record_authorised(&record(&k[0], None), None),
            RecordAuthority::Authorised
        );
        let empty = State::inception(inception());
        assert_eq!(
            record_authorised(&record(&k[0], None), Some(&empty)),
            RecordAuthority::Authorised
        );
        assert_eq!(
            record_authorised(&record(&k[1], None), None),
            RecordAuthority::Refused(Refusal::BadSignature)
        );
    }

    #[test]
    fn record_by_the_current_key_is_accepted_and_by_a_superseded_key_refused() {
        let k = keys(4);
        let log = chain(2);
        let st = log.verify(&inception()).unwrap();
        // The record has the full seven days left, and is still refused.
        assert_eq!(
            record_authorised(&record(&k[2], None), Some(&st)),
            RecordAuthority::Authorised
        );
        assert_eq!(
            record_authorised(&record(&k[0], None), Some(&st)),
            RecordAuthority::Refused(Refusal::SupersededKey)
        );
        assert_eq!(
            record_authorised(&record(&k[1], None), Some(&st)),
            RecordAuthority::Refused(Refusal::SupersededKey)
        );
        // A key that never was current is a plain bad signature, and so is the
        // committed-but-not-yet-revealed next key.
        assert_eq!(
            record_authorised(&record(&k[3], None), Some(&st)),
            RecordAuthority::Refused(Refusal::BadSignature)
        );
        assert_eq!(
            record_authorised(&record(&sk(50), None), Some(&st)),
            RecordAuthority::Refused(Refusal::BadSignature)
        );
    }

    #[test]
    fn a_verifier_without_the_entry_still_accepts_the_old_key() {
        // The exposure the spec states: held log is empty, record is by key 1.
        let k = keys(1);
        let held = State::inception(inception());
        assert_eq!(
            record_authorised(&record(&k[0], None), Some(&held)),
            RecordAuthority::Authorised
        );
    }

    #[test]
    fn a_record_claiming_a_newer_head_needs_the_log() {
        let k = keys(3);
        let log = chain(2);
        let st1 = SuccessionLog {
            entries: log.entries[..1].to_vec(),
        }
        .verify(&inception())
        .unwrap();
        let st2 = log.verify(&inception()).unwrap();
        let claim2 = st2.head_claim().unwrap();
        assert_eq!(
            record_authorised(&record(&k[2], Some(claim2)), Some(&st1)),
            RecordAuthority::NeedLog {
                claimed: claim2,
                held: 1
            }
        );
        // With no log at all the same record also needs one.
        assert_eq!(
            record_authorised(&record(&k[2], Some(claim2)), None),
            RecordAuthority::NeedLog {
                claimed: claim2,
                held: 0
            }
        );
        // Once the log is held it is accepted.
        assert_eq!(
            record_authorised(&record(&k[2], Some(claim2)), Some(&st2)),
            RecordAuthority::Authorised
        );
    }

    #[test]
    fn claims_below_or_beside_the_held_head_are_refused() {
        let k = keys(3);
        let log = chain(2);
        let st1 = SuccessionLog {
            entries: log.entries[..1].to_vec(),
        }
        .verify(&inception())
        .unwrap();
        let st2 = log.verify(&inception()).unwrap();
        assert_eq!(
            record_authorised(&record(&k[2], st1.head_claim()), Some(&st2)),
            RecordAuthority::Refused(Refusal::StaleClaim {
                claimed: 1,
                held: 2
            })
        );
        let wrong = HeadClaim {
            index: 2,
            head: [7u8; 32],
        };
        assert_eq!(
            record_authorised(&record(&k[2], Some(wrong)), Some(&st2)),
            RecordAuthority::Refused(Refusal::HeadConflict)
        );
    }

    #[test]
    fn malformed_claim_and_foreign_name_are_refused() {
        let k = keys(1);
        let mut r = record(&k[0], None).with_meta(SUCCESSION_META_KEY, "garbage");
        let sig: Signature = k[0].sign(&r.signing_bytes());
        r.signature = sig.to_bytes().to_vec();
        assert_eq!(
            record_authorised(&r, None),
            RecordAuthority::Refused(Refusal::MalformedClaim)
        );

        let other = SpineUri::did(pk(&sk(40)));
        let foreign = NameRecord::new(other, 1, 1_000).unwrap();
        let st = State::inception(inception());
        assert_eq!(
            record_authorised(&foreign, Some(&st)),
            RecordAuthority::Refused(Refusal::WrongName)
        );
        let host =
            NameRecord::for_host(SpineUri::parse("spine://host:h:1/").unwrap(), 1, 60).unwrap();
        assert!(matches!(
            record_authorised(&host, None),
            RecordAuthority::Refused(Refusal::Record(NameError::NotSignable(_)))
        ));
    }

    #[test]
    fn a_retired_name_authorises_nothing() {
        let k = keys(3);
        let mut log = chain(1);
        let r = retire(&log, &k[1], &k[2], 200);
        log.entries.push(r);
        let st = log.verify(&inception()).unwrap();
        for key in &k {
            assert_eq!(
                record_authorised(&record(key, None), Some(&st)),
                RecordAuthority::Refused(Refusal::Retired)
            );
        }
        // Even a claim at the retire head does not help.
        assert_eq!(
            record_authorised(&record(&k[1], st.head_claim()), Some(&st)),
            RecordAuthority::Refused(Refusal::Retired)
        );
    }

    #[test]
    fn record_rules_still_apply_under_the_current_key() {
        let k = keys(2);
        let log = chain(1);
        let st = log.verify(&inception()).unwrap();
        let mut r = NameRecord::new(name(), 1, 1_000).unwrap();
        r.ttl_secs = crate::MAX_RECORD_TTL_SECS + 1;
        let sig: Signature = k[1].sign(&r.signing_bytes());
        r.signature = sig.to_bytes().to_vec();
        assert!(matches!(
            record_authorised(&r, Some(&st)),
            RecordAuthority::Refused(Refusal::Record(NameError::TtlTooLong(_)))
        ));
    }

    #[test]
    fn verify_with_the_name_key_is_verify() {
        let k = sk(1);
        let mut r = NameRecord::new(name(), 1, 1_000).unwrap();
        r.sign(&k).unwrap();
        assert_eq!(r.verify_with(&inception()), r.verify());
        assert_eq!(r.verify_with(&pk(&sk(2))), Err(NameError::BadSignature));
    }
}
