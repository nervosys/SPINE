//! Grants and chains: the types, their one canonical encoding, signing and
//! decoding.
//!
//! # Encoding
//!
//! A grant is a [`GrantBody`] plus a signature. Both are deterministic CBOR
//! (`spine-cbor`), a map with text keys. Keys, hashes, nonces and signatures
//! are CBOR byte strings.
//!
//! * The bytes that are signed are [`GRANT_DOMAIN`] followed by the CBOR of the
//!   body, that is, of the grant without its signature.
//! * The encoding of a whole grant is the CBOR of a map with two entries:
//!   `body` (the body map) and `sig` (64 bytes).
//! * The id of a grant ([`Grant::id`]) is SHA-256 over [`GRANT_ID_DOMAIN`]
//!   followed by the whole encoding. It is the parent reference of a child and
//!   the revocation handle.
//!
//! # Body fields
//!
//! | Key | CBOR type | Meaning |
//! | --- | --- | --- |
//! | `v` | unsigned integer | format version, 1 |
//! | `iss` | byte string, 32 | signer, an Ed25519 public key |
//! | `aud` | byte string, 32 | the only party that may use or delegate it |
//! | `parent` | `null` or byte string, 32 | id of the parent grant; `null` only in a root |
//! | `role` | text | audit label: `principal`, `operator`, `agent`, `service` |
//! | `res` | array of maps | resource patterns, each `{owner, path, prefix}` |
//! | `act` | array of text | actions, strictly ascending |
//! | `effect` | text | `read`, `reversible` or `irreversible` |
//! | `delegable` | unsigned integer | how many more links may follow |
//! | `nbf`, `exp` | unsigned integer | validity window, Unix seconds, `nbf < exp` |
//! | `limits` | map | `{calls, cost, bytes, rate}`, each an integer or `null` |
//! | `caveats` | array of maps | `{name, arg}`, `arg` a byte string |
//! | `nonce` | byte string, 16 | makes otherwise identical grants distinct |
//!
//! Decoding refuses anything that is not exactly the canonical encoding of the
//! value it decodes to: non-deterministic CBOR, an unknown or missing field, a
//! number where bytes belong, and inputs over the bounds below.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use spine_cbor::CborError;

use crate::attenuation::{body_is_attenuation_of, Broadening};
use crate::scope::ResourcePattern;

/// The grant format version this crate reads and writes.
pub const GRANT_VERSION: u64 = 1;

/// Prefix of the signed bytes of a grant.
pub const GRANT_DOMAIN: &[u8] = b"spine-authority-grant-v1\0";

/// Prefix of the hash that is a grant's id.
pub const GRANT_ID_DOMAIN: &[u8] = b"spine-authority-grant-id-v1\0";

/// Most grants in a chain.
pub const MAX_CHAIN_DEPTH: usize = 8;

/// Longest encoded grant, in bytes.
pub const MAX_GRANT_BYTES: usize = 4096;

/// Longest encoded chain, in bytes: the grants plus the array head.
pub const MAX_CHAIN_BYTES: usize = MAX_CHAIN_DEPTH * MAX_GRANT_BYTES + 8;

/// Most resource patterns in one grant.
pub const MAX_RESOURCES: usize = 16;

/// Most caveats in one grant.
pub const MAX_CAVEATS: usize = 8;

/// Longest caveat name, in bytes.
pub const MAX_CAVEAT_NAME_LEN: usize = 64;

/// Longest caveat argument, in bytes.
pub const MAX_CAVEAT_ARG_LEN: usize = 256;

/// Longest role label, in bytes.
pub const MAX_ROLE_LEN: usize = 16;

/// The audit labels a grant may carry.
pub const KNOWN_ROLES: &[&str] = &["principal", "operator", "agent", "service"];

/// The caveat names this version recognises. It is empty: the draft design
/// registers no caveat, so every grant that carries one is refused at step 2
/// of verification. See "Provisional decisions" in `docs/spec/03-authority.md`.
pub const KNOWN_CAVEATS: &[&str] = &[];

/// A 32-byte Ed25519 public key.
pub type PublicKey = [u8; 32];

/// The id of a grant: see [`Grant::id`].
pub type GrantId = [u8; 32];

/// What an invocation does to a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// Read.
    Read,
    /// Invoke an operation.
    Invoke,
    /// Publish.
    Publish,
    /// Administer.
    Admin,
}

impl Action {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Action::Read => "read",
            Action::Invoke => "invoke",
            Action::Publish => "publish",
            Action::Admin => "admin",
        }
    }

    /// The action with this wire spelling, if any.
    pub fn parse(text: &str) -> Option<Action> {
        match text {
            "read" => Some(Action::Read),
            "invoke" => Some(Action::Invoke),
            "publish" => Some(Action::Publish),
            "admin" => Some(Action::Admin),
            _ => None,
        }
    }
}

impl Serialize for Action {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Action {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Action, D::Error> {
        let text = String::deserialize(deserializer)?;
        match Action::parse(&text) {
            Some(action) => Ok(action),
            None => Err(serde::de::Error::custom("unknown action")),
        }
    }
}

/// The ceiling on the effect class of an operation (layer 5). Ordered:
/// `Read < Reversible < Irreversible`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    /// No change to the world.
    Read,
    /// A change that can be undone.
    Reversible,
    /// A change that cannot be undone.
    Irreversible,
}

impl Effect {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Effect::Read => "read",
            Effect::Reversible => "reversible",
            Effect::Irreversible => "irreversible",
        }
    }

    /// The effect class with this wire spelling, if any.
    pub fn parse(text: &str) -> Option<Effect> {
        match text {
            "read" => Some(Effect::Read),
            "reversible" => Some(Effect::Reversible),
            "irreversible" => Some(Effect::Irreversible),
            _ => None,
        }
    }
}

impl Serialize for Effect {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Effect {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Effect, D::Error> {
        let text = String::deserialize(deserializer)?;
        match Effect::parse(&text) {
            Some(effect) => Ok(effect),
            None => Err(serde::de::Error::custom("unknown effect class")),
        }
    }
}

/// One of the four limits a grant can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitKind {
    /// Number of invocations.
    Calls,
    /// Declared cost, in layer-5 units (undefined yet).
    Cost,
    /// Bytes moved.
    Bytes,
    /// Invocations per rolling window of [`crate::RATE_WINDOW_SECS`] seconds.
    Rate,
}

impl LimitKind {
    /// Every kind, in a fixed order.
    pub const ALL: [LimitKind; 4] = [
        LimitKind::Calls,
        LimitKind::Cost,
        LimitKind::Bytes,
        LimitKind::Rate,
    ];
}

/// Optional ceilings. `None` means the grant sets no ceiling of that kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Most invocations, over the life of the grant.
    pub calls: Option<u64>,
    /// Most declared cost, over the life of the grant.
    pub cost: Option<u64>,
    /// Most bytes, over the life of the grant.
    pub bytes: Option<u64>,
    /// Most invocations in any rolling window.
    pub rate: Option<u64>,
}

impl Limits {
    /// The ceiling of one kind.
    pub fn get(&self, kind: LimitKind) -> Option<u64> {
        match kind {
            LimitKind::Calls => self.calls,
            LimitKind::Cost => self.cost,
            LimitKind::Bytes => self.bytes,
            LimitKind::Rate => self.rate,
        }
    }

    /// `true` when at least one ceiling is set.
    pub fn any(&self) -> bool {
        LimitKind::ALL.iter().any(|k| self.get(*k).is_some())
    }
}

/// A further restriction from a registered set. The set is empty in this
/// version ([`KNOWN_CAVEATS`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caveat {
    /// The registered name.
    pub name: String,
    /// Opaque argument, meaning fixed by the registration.
    #[serde(with = "crate::bytes::variable")]
    pub arg: Vec<u8>,
}

/// Why a grant body is not well formed (verification step 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Malformation {
    /// `v` is not [`GRANT_VERSION`].
    #[error("unknown grant version")]
    UnknownVersion,
    /// `role` is not one of [`KNOWN_ROLES`].
    #[error("unknown role label")]
    UnknownRole,
    /// `res` is empty.
    #[error("a grant must name at least one resource")]
    EmptyResources,
    /// A resource pattern is not in canonical segment form.
    #[error("a resource pattern is not in canonical form")]
    BadScope,
    /// `act` is empty.
    #[error("a grant must allow at least one action")]
    EmptyActions,
    /// `act` is not strictly ascending, so it is not a canonical set.
    #[error("actions are not a strictly ascending set")]
    ActionsNotCanonical,
    /// `nbf` is not before `exp`.
    #[error("the validity window is empty")]
    BadWindow,
    /// A caveat name is not in [`KNOWN_CAVEATS`].
    #[error("unrecognised caveat")]
    UnknownCaveat,
}

/// The signed content of a grant. See the module documentation for the field
/// table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantBody {
    /// Format version.
    #[serde(rename = "v")]
    pub version: u64,
    /// Signer.
    #[serde(with = "crate::bytes::fixed")]
    pub iss: PublicKey,
    /// The party the grant is for.
    #[serde(with = "crate::bytes::fixed")]
    pub aud: PublicKey,
    /// The parent grant's id, or `None` in a root grant.
    #[serde(with = "crate::bytes::optional_id")]
    pub parent: Option<GrantId>,
    /// Audit label for `aud`.
    pub role: String,
    /// Resources the grant covers.
    pub res: Vec<ResourcePattern>,
    /// Actions the grant allows, strictly ascending.
    pub act: Vec<Action>,
    /// Ceiling on the effect class.
    pub effect: Effect,
    /// How many further links may follow this grant.
    pub delegable: u32,
    /// Not valid before this Unix time (seconds).
    pub nbf: u64,
    /// Not valid at or after this Unix time (seconds).
    pub exp: u64,
    /// Optional ceilings.
    pub limits: Limits,
    /// Further restrictions.
    pub caveats: Vec<Caveat>,
    /// Makes otherwise identical grants distinct.
    #[serde(with = "crate::bytes::fixed")]
    pub nonce: [u8; 16],
}

impl GrantBody {
    /// The bytes the issuer signs: [`GRANT_DOMAIN`] then the deterministic CBOR
    /// of the body.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, CborError> {
        let mut out: Vec<u8> = GRANT_DOMAIN.to_vec();
        let body = spine_cbor::to_vec(self)?;
        out.extend_from_slice(body.as_slice());
        Ok(out)
    }

    /// Checks the explicit size bounds of this crate. This is verification
    /// step 1 for a single grant, and decoding refuses a grant that fails it.
    pub fn check_bounds(&self) -> Result<(), &'static str> {
        if self.res.len() > MAX_RESOURCES {
            return Err("too many resource patterns");
        }
        if self.act.len() > 4 {
            return Err("too many actions");
        }
        if self.caveats.len() > MAX_CAVEATS {
            return Err("too many caveats");
        }
        if self.role.len() > MAX_ROLE_LEN {
            return Err("role label too long");
        }
        for caveat in &self.caveats {
            if caveat.name.len() > MAX_CAVEAT_NAME_LEN {
                return Err("caveat name too long");
            }
            if caveat.arg.len() > MAX_CAVEAT_ARG_LEN {
                return Err("caveat argument too long");
            }
        }
        for pattern in &self.res {
            if pattern.path.len() > crate::scope::MAX_PATH_SEGMENTS {
                return Err("too many path segments");
            }
            if pattern
                .path
                .iter()
                .any(|s| s.len() > crate::scope::MAX_SEGMENT_LEN)
            {
                return Err("path segment too long");
            }
        }
        Ok(())
    }

    /// Checks everything that makes a body acceptable apart from its links
    /// and signature: version, role, scopes, actions, window and caveats.
    /// This is verification step 2 for a single grant.
    pub fn check_well_formed(&self) -> Result<(), Malformation> {
        if self.version != GRANT_VERSION {
            return Err(Malformation::UnknownVersion);
        }
        if !KNOWN_ROLES.contains(&self.role.as_str()) {
            return Err(Malformation::UnknownRole);
        }
        if self.res.is_empty() {
            return Err(Malformation::EmptyResources);
        }
        for pattern in &self.res {
            if pattern.validate().is_err() {
                return Err(Malformation::BadScope);
            }
        }
        if self.act.is_empty() {
            return Err(Malformation::EmptyActions);
        }
        if self.act.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(Malformation::ActionsNotCanonical);
        }
        if self.nbf >= self.exp {
            return Err(Malformation::BadWindow);
        }
        for caveat in &self.caveats {
            if !KNOWN_CAVEATS.contains(&caveat.name.as_str()) {
                return Err(Malformation::UnknownCaveat);
            }
        }
        Ok(())
    }
}

/// Why bytes are not an acceptable encoding.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// The input is longer than the limit (given in bytes).
    #[error("encoding is longer than {0} bytes")]
    TooLarge(usize),
    /// Not deterministic CBOR, or not the expected shape: a wrong type, an
    /// unknown field, a missing field, a wrong length.
    #[error("cbor: {0}")]
    Cbor(CborError),
    /// Valid CBOR of the right shape that is not the canonical encoding of the
    /// value it decodes to (for example a field left out where `null` belongs).
    #[error("not the canonical encoding of its value")]
    NonCanonical,
    /// A structural bound was exceeded.
    #[error("bound exceeded: {0}")]
    Bounds(&'static str),
}

/// Why a grant could not be issued.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IssueError {
    /// The grant would exceed a structural bound.
    #[error("bound exceeded: {0}")]
    Bounds(&'static str),
    /// The grant is not well formed.
    #[error("malformed grant: {0}")]
    Malformed(Malformation),
    /// A root grant names a resource the signer does not own.
    #[error("a root grant may only name resources its issuer owns")]
    NotOwner,
    /// A delegated grant is broader than its parent.
    #[error("grant would broaden its parent: {0}")]
    Broadens(Broadening),
    /// Encoding failed.
    #[error("cbor: {0}")]
    Cbor(CborError),
}

/// What an issuer chooses when issuing a grant. The issuer, the parent
/// reference, the version and the signature are filled in by
/// [`Grant::issue_root`] and [`Grant::delegate`]; there is no clock and no
/// random source, so the validity window and the nonce are the caller's.
#[derive(Debug, Clone)]
pub struct GrantSpec {
    /// The party the grant is for.
    pub aud: PublicKey,
    /// Audit label for `aud`.
    pub role: String,
    /// Resources the grant covers.
    pub res: Vec<ResourcePattern>,
    /// Actions allowed. Sorted and de-duplicated by the builders.
    pub act: Vec<Action>,
    /// Ceiling on the effect class.
    pub effect: Effect,
    /// How many more links may follow.
    pub delegable: u32,
    /// Not valid before.
    pub nbf: u64,
    /// Not valid at or after.
    pub exp: u64,
    /// Optional ceilings.
    pub limits: Limits,
    /// Further restrictions.
    pub caveats: Vec<Caveat>,
    /// Distinguishes otherwise identical grants.
    pub nonce: [u8; 16],
}

/// A signed grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// The signed content.
    pub body: GrantBody,
    /// Ed25519 signature by `body.iss` over [`GrantBody::signing_bytes`].
    #[serde(rename = "sig", with = "crate::bytes::fixed")]
    pub signature: [u8; 64],
}

impl Grant {
    /// Signs `body` as it is. No shape, ownership or attenuation check is made:
    /// this exists to build hostile and test inputs. Anything made with it that
    /// is not valid is refused by [`crate::verify`]. Use
    /// [`Grant::issue_root`] and [`Grant::delegate`] to issue real grants.
    pub fn sign_unchecked(signer: &SigningKey, body: GrantBody) -> Result<Grant, CborError> {
        let message = body.signing_bytes()?;
        let signature: Signature = signer.sign(message.as_slice());
        Ok(Grant {
            body,
            signature: signature.to_bytes(),
        })
    }

    /// The canonical encoding of the whole grant.
    pub fn encode(&self) -> Result<Vec<u8>, CborError> {
        spine_cbor::to_vec(self)
    }

    /// The id of the grant: SHA-256 over [`GRANT_ID_DOMAIN`] and the encoding.
    pub fn id(&self) -> Result<GrantId, CborError> {
        let encoded = self.encode()?;
        let mut hasher = Sha256::new();
        hasher.update(GRANT_ID_DOMAIN);
        hasher.update(encoded.as_slice());
        let digest: [u8; 32] = hasher.finalize().into();
        Ok(digest)
    }

    /// Decodes one grant. The input must be exactly the canonical encoding and
    /// within [`MAX_GRANT_BYTES`] and the structural bounds. Version, role,
    /// scope and caveat checks are not made here; they are verification
    /// step 2 ([`GrantBody::check_well_formed`]).
    pub fn decode(bytes: &[u8]) -> Result<Grant, DecodeError> {
        if bytes.len() > MAX_GRANT_BYTES {
            return Err(DecodeError::TooLarge(MAX_GRANT_BYTES));
        }
        let grant: Grant = spine_cbor::from_slice(bytes).map_err(DecodeError::Cbor)?;
        let again = spine_cbor::to_vec(&grant).map_err(DecodeError::Cbor)?;
        if again.as_slice() != bytes {
            return Err(DecodeError::NonCanonical);
        }
        grant.body.check_bounds().map_err(DecodeError::Bounds)?;
        Ok(grant)
    }

    /// Whether the signature is a valid strict Ed25519 signature by `iss`
    /// over the signed bytes. `false` for any failure, including an `iss` that
    /// is not a valid curve point.
    pub fn verify_signature(&self) -> bool {
        let key = match VerifyingKey::from_bytes(&self.body.iss) {
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

    fn finish(signer: &SigningKey, body: GrantBody) -> Result<Grant, IssueError> {
        body.check_bounds().map_err(IssueError::Bounds)?;
        body.check_well_formed().map_err(IssueError::Malformed)?;
        let grant = Grant::sign_unchecked(signer, body).map_err(IssueError::Cbor)?;
        let encoded = grant.encode().map_err(IssueError::Cbor)?;
        if encoded.len() > MAX_GRANT_BYTES {
            return Err(IssueError::Bounds("encoded grant too long"));
        }
        Ok(grant)
    }

    fn body_from(issuer: PublicKey, parent: Option<GrantId>, mut spec: GrantSpec) -> GrantBody {
        spec.act.sort();
        spec.act.dedup();
        GrantBody {
            version: GRANT_VERSION,
            iss: issuer,
            aud: spec.aud,
            parent,
            role: spec.role,
            res: spec.res,
            act: spec.act,
            effect: spec.effect,
            delegable: spec.delegable,
            nbf: spec.nbf,
            exp: spec.exp,
            limits: spec.limits,
            caveats: spec.caveats,
            nonce: spec.nonce,
        }
    }

    /// Issues a root grant. The signer is the issuer and must own every
    /// resource the grant names. The grant must be well formed and within
    /// bounds.
    pub fn issue_root(signer: &SigningKey, spec: GrantSpec) -> Result<Grant, IssueError> {
        let issuer: PublicKey = signer.verifying_key().to_bytes();
        if spec.res.iter().any(|pattern| pattern.owner != issuer) {
            return Err(IssueError::NotOwner);
        }
        let body = Grant::body_from(issuer, None, spec);
        Grant::finish(signer, body)
    }

    /// Delegates `parent` to `spec.aud`. The signer must be the parent's
    /// audience, and the new grant must be an attenuation of the parent
    /// ([`crate::is_attenuation_of`]); a grant that broadens it is refused and
    /// nothing is signed.
    pub fn delegate(
        signer: &SigningKey,
        parent: &Grant,
        spec: GrantSpec,
    ) -> Result<Grant, IssueError> {
        let issuer: PublicKey = signer.verifying_key().to_bytes();
        let parent_id = parent.id().map_err(IssueError::Cbor)?;
        let body = Grant::body_from(issuer, Some(parent_id), spec);
        body_is_attenuation_of(&body, &parent.body).map_err(IssueError::Broadens)?;
        Grant::finish(signer, body)
    }
}

/// An ordered list of grants, root first, in which each grant's issuer is the
/// previous grant's audience.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    /// The grants, root first.
    pub grants: Vec<Grant>,
}

impl Chain {
    /// A chain of these grants (not checked).
    pub fn new(grants: Vec<Grant>) -> Chain {
        Chain { grants }
    }

    /// The first grant.
    pub fn root(&self) -> Option<&Grant> {
        self.grants.first()
    }

    /// The last grant; its audience is the party that may invoke.
    pub fn leaf(&self) -> Option<&Grant> {
        self.grants.last()
    }

    /// Number of grants.
    pub fn len(&self) -> usize {
        self.grants.len()
    }

    /// `true` when there are no grants.
    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }

    /// The canonical encoding: a CBOR array of the encoded grants.
    pub fn encode(&self) -> Result<Vec<u8>, CborError> {
        spine_cbor::to_vec(&self.grants)
    }

    /// Decodes a chain. Same rules as [`Grant::decode`] for each grant, plus
    /// [`MAX_CHAIN_BYTES`] and [`MAX_CHAIN_DEPTH`], and a chain must not be
    /// empty.
    pub fn decode(bytes: &[u8]) -> Result<Chain, DecodeError> {
        if bytes.len() > MAX_CHAIN_BYTES {
            return Err(DecodeError::TooLarge(MAX_CHAIN_BYTES));
        }
        let grants: Vec<Grant> = spine_cbor::from_slice(bytes).map_err(DecodeError::Cbor)?;
        let again = spine_cbor::to_vec(&grants).map_err(DecodeError::Cbor)?;
        if again.as_slice() != bytes {
            return Err(DecodeError::NonCanonical);
        }
        if grants.is_empty() {
            return Err(DecodeError::Bounds("empty chain"));
        }
        if grants.len() > MAX_CHAIN_DEPTH {
            return Err(DecodeError::Bounds("chain too long"));
        }
        for grant in &grants {
            grant.body.check_bounds().map_err(DecodeError::Bounds)?;
            let encoded = grant.encode().map_err(DecodeError::Cbor)?;
            if encoded.len() > MAX_GRANT_BYTES {
                return Err(DecodeError::TooLarge(MAX_GRANT_BYTES));
            }
        }
        Ok(Chain { grants })
    }
}
