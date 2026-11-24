//! Invocations: the request that is verified against a chain.
//!
//! An invocation says who is acting (`invoker`), toward whom (`recipient`),
//! on which session, when, which action and effect class on which resources,
//! and which chain it cites (`leaf`, the id of the last grant). It is signed by
//! the invoker over [`INVOCATION_DOMAIN`] followed by the deterministic CBOR of
//! the body.
//!
//! | Key | CBOR type | Meaning |
//! | --- | --- | --- |
//! | `v` | unsigned integer | format version, 1 |
//! | `invoker` | byte string, 32 | the acting key; must be the last grant's audience |
//! | `recipient` | byte string, 32 | the party that will perform it |
//! | `session` | byte string, 32 | binding to the layer-1 session it is sent on |
//! | `nonce` | byte string, 16 | single use, per invoker |
//! | `time` | unsigned integer | when it was made, Unix seconds |
//! | `res` | array of maps | resources touched, each `{owner, path}` |
//! | `act` | text | the action |
//! | `effect` | text | the declared effect class |
//! | `op` | byte string, 32 | hash of the operation and its arguments |
//! | `leaf` | byte string, 32 | id of the last grant of the chain |
//! | `cost` | unsigned integer | declared cost to charge against `cost` limits |
//! | `bytes` | unsigned integer | declared bytes to charge against `bytes` limits |

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spine_cbor::CborError;

use crate::grant::{Action, DecodeError, Effect, GrantId, PublicKey};
use crate::scope::ResourceName;

/// The invocation format version this crate reads and writes.
pub const INVOCATION_VERSION: u64 = 1;

/// Prefix of the signed bytes of an invocation.
pub const INVOCATION_DOMAIN: &[u8] = b"spine-authority-invocation-v1\0";

/// Prefix of the hash that is an invocation's id.
pub const INVOCATION_ID_DOMAIN: &[u8] = b"spine-authority-invocation-id-v1\0";

/// The id of an invocation: see [`Invocation::id`].
pub type InvocationId = [u8; 32];

/// Most resources one invocation may touch.
pub const MAX_INVOCATION_RESOURCES: usize = 16;

/// Longest encoded invocation, in bytes.
pub const MAX_INVOCATION_BYTES: usize = 4096;

/// The signed content of an invocation. See the module documentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationBody {
    /// Format version.
    #[serde(rename = "v")]
    pub version: u64,
    /// The acting key.
    #[serde(with = "crate::bytes::fixed")]
    pub invoker: PublicKey,
    /// The party that will perform the operation.
    #[serde(with = "crate::bytes::fixed")]
    pub recipient: PublicKey,
    /// Binding to the layer-1 session.
    #[serde(with = "crate::bytes::fixed")]
    pub session: [u8; 32],
    /// Single-use value, so the invocation cannot be replayed.
    #[serde(with = "crate::bytes::fixed")]
    pub nonce: [u8; 16],
    /// When the invocation was made, Unix seconds.
    pub time: u64,
    /// The resources the operation touches.
    pub res: Vec<ResourceName>,
    /// The action.
    pub act: Action,
    /// The declared effect class.
    pub effect: Effect,
    /// Hash of the operation and its arguments.
    #[serde(with = "crate::bytes::fixed")]
    pub op: [u8; 32],
    /// Id of the last grant of the chain this invocation cites.
    #[serde(with = "crate::bytes::fixed")]
    pub leaf: GrantId,
    /// Declared cost.
    pub cost: u64,
    /// Declared bytes.
    pub bytes: u64,
}

impl InvocationBody {
    /// The bytes the invoker signs.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, CborError> {
        let mut out: Vec<u8> = INVOCATION_DOMAIN.to_vec();
        let body = spine_cbor::to_vec(self)?;
        out.extend_from_slice(body.as_slice());
        Ok(out)
    }
}

/// A signed invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    /// The signed content.
    pub body: InvocationBody,
    /// Ed25519 signature by `body.invoker`.
    #[serde(rename = "sig", with = "crate::bytes::fixed")]
    pub signature: [u8; 64],
}

impl Invocation {
    /// Signs `body` with `signer`. The signer should be `body.invoker`; if it
    /// is not, the result fails verification at step 9.
    pub fn sign(signer: &SigningKey, body: InvocationBody) -> Result<Invocation, CborError> {
        let message = body.signing_bytes()?;
        let signature: Signature = signer.sign(message.as_slice());
        Ok(Invocation {
            body,
            signature: signature.to_bytes(),
        })
    }

    /// The canonical encoding.
    pub fn encode(&self) -> Result<Vec<u8>, CborError> {
        spine_cbor::to_vec(self)
    }

    /// The id of the invocation: SHA-256 over [`INVOCATION_ID_DOMAIN`] and the
    /// encoding of the whole signed invocation (the same convention as
    /// [`crate::Grant::id`]). Receipts cite it.
    pub fn id(&self) -> Result<InvocationId, CborError> {
        let encoded = self.encode()?;
        let mut hasher = Sha256::new();
        hasher.update(INVOCATION_ID_DOMAIN);
        hasher.update(encoded.as_slice());
        let digest: [u8; 32] = hasher.finalize().into();
        Ok(digest)
    }

    /// Decodes an invocation: exactly the canonical encoding, within
    /// [`MAX_INVOCATION_BYTES`] and [`MAX_INVOCATION_RESOURCES`].
    pub fn decode(bytes: &[u8]) -> Result<Invocation, DecodeError> {
        if bytes.len() > MAX_INVOCATION_BYTES {
            return Err(DecodeError::TooLarge(MAX_INVOCATION_BYTES));
        }
        let invocation: Invocation = spine_cbor::from_slice(bytes).map_err(DecodeError::Cbor)?;
        let again = spine_cbor::to_vec(&invocation).map_err(DecodeError::Cbor)?;
        if again.as_slice() != bytes {
            return Err(DecodeError::NonCanonical);
        }
        if invocation.body.res.len() > MAX_INVOCATION_RESOURCES {
            return Err(DecodeError::Bounds("too many resources"));
        }
        Ok(invocation)
    }

    /// Whether the signature is a valid strict Ed25519 signature by `invoker`.
    pub fn verify_signature(&self) -> bool {
        let key = match VerifyingKey::from_bytes(&self.body.invoker) {
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

    /// `true` when the version is known, at least one resource is named, and
    /// every resource name is in canonical form.
    pub fn is_well_formed(&self) -> bool {
        self.body.version == INVOCATION_VERSION
            && !self.body.res.is_empty()
            && self.body.res.iter().all(|r| r.validate().is_ok())
    }
}
