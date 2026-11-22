//! Receipts: what the party that performed an invocation signs afterwards.
//!
//! A receipt is signed by the invocation's recipient, the party that checked
//! it and, if it was allowed, did the work. The signed bytes are
//! [`RECEIPT_DOMAIN`] followed by the deterministic CBOR of the body; the
//! encoding of a whole receipt is the CBOR of `{body, sig}`.
//!
//! | Key | CBOR type | Meaning |
//! | --- | --- | --- |
//! | `v` | unsigned integer | format version, 1 |
//! | `recipient` | byte string, 32 | the signer: the party that performed the invocation |
//! | `invocation` | byte string, 32 | [`Invocation::id`] of the signed invocation |
//! | `leaf` | byte string, 32 | id of the last grant, as the invocation cites it |
//! | `allowed` | boolean | `true` if the invocation was allowed and performed |
//! | `effect` | text or null | the effect class that was performed; null when denied |
//! | `step` | unsigned integer or null | the verification step that refused, 1 to 11; null when allowed |
//! | `calls` | unsigned integer | charged against `calls` limits: 1 when allowed, 0 when denied |
//! | `cost` | unsigned integer | charged against `cost` limits; 0 when denied |
//! | `bytes` | unsigned integer | charged against `bytes` limits; 0 when denied |
//! | `result` | byte string, 32 | digest of the result, opaque to this crate; all zero when denied |
//! | `time` | unsigned integer | when it was performed or refused, Unix seconds |
//!
//! A denial receipt carries the step number and nothing else about why: no
//! variant, no grant, no amounts and a zero result. An allowed receipt and a
//! denial receipt are distinguished by `allowed`, and the fields that do not
//! apply to each are fixed, so there is one encoding of each meaning.
//!
//! With the chain, the invocation and the receipt, a third party can check who
//! delegated, who acted, what was done and what it was charged, without
//! trusting either side: the grants are signed, the invocation is signed and
//! the receipt names the invocation by hash.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use spine_cbor::CborError;

use crate::grant::{DecodeError, Effect, GrantId, PublicKey};
use crate::invocation::{Invocation, InvocationId};
use crate::verification::{Decision, Denial};

/// The receipt format version this crate reads and writes.
pub const RECEIPT_VERSION: u64 = 1;

/// Prefix of the signed bytes of a receipt.
pub const RECEIPT_DOMAIN: &[u8] = b"spine-authority-receipt-v1\0";

/// Longest encoded receipt, in bytes.
pub const MAX_RECEIPT_BYTES: usize = 512;

/// The highest verification step number.
const LAST_STEP: u64 = 11;

/// The signed content of a receipt. See the module documentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptBody {
    /// Format version.
    #[serde(rename = "v")]
    pub version: u64,
    /// The signer, the party that performed the invocation.
    #[serde(with = "crate::bytes::fixed")]
    pub recipient: PublicKey,
    /// Id of the invocation this receipt is for.
    #[serde(with = "crate::bytes::fixed")]
    pub invocation: InvocationId,
    /// Id of the last grant, as the invocation cites it.
    #[serde(with = "crate::bytes::fixed")]
    pub leaf: GrantId,
    /// Whether the invocation was allowed and performed.
    pub allowed: bool,
    /// The effect class performed. `None` when denied.
    pub effect: Option<Effect>,
    /// The verification step that refused. `None` when allowed.
    pub step: Option<u64>,
    /// Calls charged: 1 when allowed, 0 when denied.
    pub calls: u64,
    /// Cost charged.
    pub cost: u64,
    /// Bytes charged.
    pub bytes: u64,
    /// Digest of the result. All zero when denied.
    #[serde(with = "crate::bytes::fixed")]
    pub result: [u8; 32],
    /// When it was performed or refused, Unix seconds.
    pub time: u64,
}

impl ReceiptBody {
    /// The bytes the recipient signs: [`RECEIPT_DOMAIN`] then the
    /// deterministic CBOR of the body.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, CborError> {
        let mut out: Vec<u8> = RECEIPT_DOMAIN.to_vec();
        let body = spine_cbor::to_vec(self)?;
        out.extend_from_slice(body.as_slice());
        Ok(out)
    }

    /// `true` when the version is known and the fields are consistent with
    /// `allowed`: see the module documentation.
    pub fn is_well_formed(&self) -> bool {
        if self.version != RECEIPT_VERSION {
            return false;
        }
        if self.allowed {
            self.effect.is_some() && self.step.is_none() && self.calls == 1
        } else {
            let step_ok = match self.step {
                Some(step) => (1..=LAST_STEP).contains(&step),
                None => false,
            };
            self.effect.is_none()
                && step_ok
                && self.calls == 0
                && self.cost == 0
                && self.bytes == 0
                && self.result == [0u8; 32]
        }
    }
}

/// Why a receipt could not be issued or did not check out.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReceiptError {
    /// Encoding failed.
    #[error("cbor: {0}")]
    Cbor(CborError),
    /// The version is unknown, or the fields are inconsistent with `allowed`.
    #[error("the receipt is malformed")]
    Malformed,
    /// The signature does not verify.
    #[error("the receipt signature does not verify")]
    BadSignature,
    /// The receipt is signed by a party other than the expected recipient.
    #[error("the receipt is from another recipient")]
    WrongRecipient,
    /// The signer is not the recipient the invocation names.
    #[error("the signer is not the recipient of the invocation")]
    NotRecipient,
    /// The decision does not belong to the invocation.
    #[error("the decision is for another invocation")]
    DecisionMismatch,
    /// The receipt is for another invocation.
    #[error("the receipt is for another invocation")]
    InvocationMismatch,
    /// The receipt names another last grant than the invocation cites.
    #[error("the receipt names another last grant")]
    LeafMismatch,
    /// The receipt's outcome or amounts differ from the decision.
    #[error("the receipt does not record this decision")]
    OutcomeMismatch,
}

/// A signed receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    /// The signed content.
    pub body: ReceiptBody,
    /// Ed25519 signature by `body.recipient`.
    #[serde(rename = "sig", with = "crate::bytes::fixed")]
    pub signature: [u8; 64],
}

/// The body a verdict on `invocation` produces, without the signer check.
fn body_for(
    invocation: &Invocation,
    verdict: &Result<Decision, Denial>,
    result: [u8; 32],
    time: u64,
) -> Result<ReceiptBody, ReceiptError> {
    let invocation_id = invocation.id().map_err(ReceiptError::Cbor)?;
    let mut body = ReceiptBody {
        version: RECEIPT_VERSION,
        recipient: invocation.body.recipient,
        invocation: invocation_id,
        leaf: invocation.body.leaf,
        allowed: false,
        effect: None,
        step: None,
        calls: 0,
        cost: 0,
        bytes: 0,
        result: [0u8; 32],
        time,
    };
    match verdict {
        Ok(decision) => {
            let belongs = decision.chain_ids.last() == Some(&invocation.body.leaf)
                && decision.invoker == invocation.body.invoker
                && decision.nonce == invocation.body.nonce;
            if !belongs {
                return Err(ReceiptError::DecisionMismatch);
            }
            body.allowed = true;
            body.effect = Some(decision.effect);
            body.calls = 1;
            body.cost = decision.cost;
            body.bytes = decision.bytes;
            body.result = result;
        }
        Err(denial) => {
            body.step = Some(u64::from(denial.step()));
        }
    }
    Ok(body)
}

impl Receipt {
    /// Signs `body` as it is. No consistency check is made: this exists to
    /// build hostile and test inputs. Use [`Receipt::issue`] for real
    /// receipts.
    pub fn sign_unchecked(signer: &SigningKey, body: ReceiptBody) -> Result<Receipt, CborError> {
        let message = body.signing_bytes()?;
        let signature: Signature = signer.sign(message.as_slice());
        Ok(Receipt {
            body,
            signature: signature.to_bytes(),
        })
    }

    /// Issues the receipt for `verdict`, the result of [`crate::verify`] on
    /// `invocation`. `signer` must be the invocation's recipient.
    ///
    /// For an allowed verdict the receipt records the effect class, one call,
    /// the decision's cost and bytes, and `result`. For a denial it records the
    /// step number only and ignores `result`.
    pub fn issue(
        signer: &SigningKey,
        invocation: &Invocation,
        verdict: &Result<Decision, Denial>,
        result: [u8; 32],
        time: u64,
    ) -> Result<Receipt, ReceiptError> {
        if signer.verifying_key().to_bytes() != invocation.body.recipient {
            return Err(ReceiptError::NotRecipient);
        }
        let body = body_for(invocation, verdict, result, time)?;
        Receipt::sign_unchecked(signer, body).map_err(ReceiptError::Cbor)
    }

    /// The canonical encoding of the whole receipt.
    pub fn encode(&self) -> Result<Vec<u8>, CborError> {
        spine_cbor::to_vec(self)
    }

    /// Decodes a receipt: exactly the canonical encoding, within
    /// [`MAX_RECEIPT_BYTES`]. Use [`Receipt::verify`] for the rest.
    pub fn decode(bytes: &[u8]) -> Result<Receipt, DecodeError> {
        if bytes.len() > MAX_RECEIPT_BYTES {
            return Err(DecodeError::TooLarge(MAX_RECEIPT_BYTES));
        }
        let receipt: Receipt = spine_cbor::from_slice(bytes).map_err(DecodeError::Cbor)?;
        let again = spine_cbor::to_vec(&receipt).map_err(DecodeError::Cbor)?;
        if again.as_slice() != bytes {
            return Err(DecodeError::NonCanonical);
        }
        Ok(receipt)
    }

    /// Whether the signature is a valid strict Ed25519 signature by
    /// `recipient`.
    pub fn verify_signature(&self) -> bool {
        let key = match VerifyingKey::from_bytes(&self.body.recipient) {
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

    /// Checks that the receipt is well formed, was made by
    /// `expected_recipient`, and carries a valid signature.
    pub fn verify(&self, expected_recipient: &PublicKey) -> Result<(), ReceiptError> {
        if !self.body.is_well_formed() {
            return Err(ReceiptError::Malformed);
        }
        if self.body.recipient != *expected_recipient {
            return Err(ReceiptError::WrongRecipient);
        }
        if !self.verify_signature() {
            return Err(ReceiptError::BadSignature);
        }
        Ok(())
    }

    /// Checks that this receipt records `verdict` on `invocation`: the same
    /// invocation id, last grant and recipient, and the outcome and amounts
    /// the verdict implies. The signature is not checked here; see
    /// [`Receipt::verify`]. The result digest is not compared: this crate does
    /// not know the result.
    pub fn matches(
        &self,
        invocation: &Invocation,
        verdict: &Result<Decision, Denial>,
    ) -> Result<(), ReceiptError> {
        let id = invocation.id().map_err(ReceiptError::Cbor)?;
        if self.body.invocation != id {
            return Err(ReceiptError::InvocationMismatch);
        }
        if self.body.leaf != invocation.body.leaf {
            return Err(ReceiptError::LeafMismatch);
        }
        if self.body.recipient != invocation.body.recipient {
            return Err(ReceiptError::WrongRecipient);
        }
        let expected = body_for(invocation, verdict, self.body.result, self.body.time)?;
        if self.body != expected {
            return Err(ReceiptError::OutcomeMismatch);
        }
        Ok(())
    }
}
