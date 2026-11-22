//! The eleven-step verification of `docs/spec/03-authority.md`.
//!
//! [`verify`] runs the steps in the order of the spec, cheap checks first, and
//! returns the first one that fails as a [`Denial`]. A denial names the step
//! and nothing about the chain.
//!
//! | Step | Check | Denial | Pure, or through |
//! | --- | --- | --- | --- |
//! | 1 | chain and invocation within length and size limits | `ChainLimits`, `InvocationLimits` | pure |
//! | 2 | every grant well formed, known version, known caveats | `Malformed`, `UnknownVersion`, `UnknownCaveat` | pure |
//! | 3 | the root's issuer owns every resource and is trusted | `RootNotAuthoritative` | pure; trusted keys in [`VerifyContext`] |
//! | 4 | issuer is previous audience, parent is previous id | `BrokenLink` | pure |
//! | 5 | every signature verifies (strict Ed25519) | `BadGrantSignature` | pure; no key succession exists in layer 2 yet |
//! | 6 | attenuation against the parent | `Broadened` | pure |
//! | 7 | inside every validity window, with stated skew | `NotYetValid`, `Expired` | pure; time is a parameter |
//! | 8 | nothing revoked, data fresh enough for the effect class | `Revoked`, `RevocationDataStale` | [`RevocationView`] |
//! | 9 | the invocation is by the last audience, for this recipient and session, fresh, signed, nonce unseen | several | pure, plus [`UsageLedger::nonce_seen`] |
//! | 10 | resource, action and effect within the last grant | `ResourceNotCovered`, `ActionNotGranted`, `EffectExceeded` | pure |
//! | 11 | stateful limits not exhausted | `LimitExhausted`, `UsageUnknown` | [`UsageLedger::spent`] |
//!
//! Verification only reads state. On success the [`Decision`] says what the
//! caller must record (nonce and charges) when it performs the operation.

use crate::attenuation::is_attenuation_of;
use crate::grant::{
    Chain, DecodeError, Effect, Grant, GrantId, Limits, Malformation, PublicKey, MAX_CHAIN_BYTES,
    MAX_CHAIN_DEPTH, MAX_GRANT_BYTES,
};
use crate::invocation::{Invocation, MAX_INVOCATION_BYTES, MAX_INVOCATION_RESOURCES};
use crate::state::{RevocationView, UsageLedger};

/// The largest clock skew a verifier may allow, in seconds. A larger value in
/// [`VerifyContext::skew_secs`] is reduced to this.
pub const MAX_CLOCK_SKEW_SECS: u64 = 300;

/// How old revocation data may be, in seconds, for each effect class.
///
/// The default is 0 for every class: only data that is current as of the
/// verifier's clock is accepted, and without data nothing is. A verifier that
/// can live with older data says so by raising a bound.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RevocationBounds {
    /// For invocations whose declared effect is `read`.
    pub read: u64,
    /// For `reversible`.
    pub reversible: u64,
    /// For `irreversible`.
    pub irreversible: u64,
}

impl RevocationBounds {
    /// The bound for one effect class.
    pub fn for_effect(&self, effect: Effect) -> u64 {
        match effect {
            Effect::Read => self.read,
            Effect::Reversible => self.reversible,
            Effect::Irreversible => self.irreversible,
        }
    }
}

/// What the verifier brings to a verification: the time, who it is, which
/// session the invocation arrived on, and its policy.
#[derive(Debug, Clone)]
pub struct VerifyContext<'a> {
    /// The verifier's clock, Unix seconds. There is no clock in this crate.
    pub now: u64,
    /// Clock skew tolerated at the start of a validity window and for an
    /// invocation dated in the future, at most [`MAX_CLOCK_SKEW_SECS`]. It is
    /// never applied to an expiry. Default 0.
    pub skew_secs: u64,
    /// This verifier's own key: the invocation must name it as recipient.
    pub recipient: PublicKey,
    /// The layer-1 session binding the invocation must carry.
    pub session: [u8; 32],
    /// Keys this verifier accepts as the root of a chain. Empty accepts none.
    pub trusted_roots: &'a [PublicKey],
    /// Staleness bounds for revocation data. Default: all 0.
    pub revocation_bounds: RevocationBounds,
    /// Oldest an invocation's `time` may be, in seconds. Default 60.
    pub max_invocation_age_secs: u64,
}

impl<'a> VerifyContext<'a> {
    /// A context with the most conservative policy: no skew, revocation data
    /// must be current, invocations at most 60 seconds old.
    pub fn new(
        now: u64,
        recipient: PublicKey,
        session: [u8; 32],
        trusted_roots: &'a [PublicKey],
    ) -> VerifyContext<'a> {
        VerifyContext {
            now,
            skew_secs: 0,
            recipient,
            session,
            trusted_roots,
            revocation_bounds: RevocationBounds::default(),
            max_invocation_age_secs: 60,
        }
    }
}

/// An accepted invocation, with what the caller must record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The ids of the chain's grants, root first. Charge every one.
    pub chain_ids: Vec<GrantId>,
    /// The limits of each grant of the chain, in the order of `chain_ids`, so
    /// that a ledger can check them all before charging any.
    pub chain_limits: Vec<Limits>,
    /// The `exp` of each grant of the chain, in the order of `chain_ids`.
    pub chain_expiries: Vec<u64>,
    /// The last time at which an invocation accepted now could still pass the
    /// freshness check of step 9: `now + skew + max_invocation_age_secs`. A
    /// ledger must remember the nonce at least until then.
    pub nonce_horizon: u64,
    /// The acting key. Record its nonce.
    pub invoker: PublicKey,
    /// The invocation's nonce.
    pub nonce: [u8; 16],
    /// The verifier's time at acceptance.
    pub now: u64,
    /// The declared effect class that was accepted.
    pub effect: Effect,
    /// Declared cost to charge.
    pub cost: u64,
    /// Declared bytes to charge.
    pub bytes: u64,
}

/// Why verification refused. One or more variants per step; [`Denial::step`]
/// gives the step number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Denial {
    /// Step 1: no grants, too many grants, or a grant over a size bound.
    #[error("step 1: chain is empty, too long, or too large")]
    ChainLimits,
    /// Step 1: the invocation is over a size bound.
    #[error("step 1: invocation is too large")]
    InvocationLimits,
    /// Step 2: a grant is not well formed, or did not parse.
    #[error("step 2: a grant is malformed")]
    Malformed,
    /// Step 2: a grant has an unknown version.
    #[error("step 2: unknown grant version")]
    UnknownVersion,
    /// Step 2: a grant carries a caveat that is not recognised.
    #[error("step 2: unrecognised caveat")]
    UnknownCaveat,
    /// Step 3: the root's issuer is not the owner of every resource, or is
    /// not a root this verifier trusts.
    #[error("step 3: the root issuer is not the owner of the resources or is not trusted")]
    RootNotAuthoritative,
    /// Step 4: a grant is not linked to the one before it.
    #[error("step 4: the chain is not linked")]
    BrokenLink,
    /// Step 5: a grant's signature does not verify.
    #[error("step 5: a grant signature does not verify")]
    BadGrantSignature,
    /// Step 6: a grant broadens its parent.
    #[error("step 6: a grant is broader than its parent")]
    Broadened,
    /// Step 7: a grant is not yet valid.
    #[error("step 7: a grant is not yet valid")]
    NotYetValid,
    /// Step 7: a grant has expired.
    #[error("step 7: a grant has expired")]
    Expired,
    /// Step 8: a grant of the chain is revoked.
    #[error("step 8: a grant is revoked")]
    Revoked,
    /// Step 8: revocation data is missing or older than the bound for the
    /// effect class.
    #[error("step 8: revocation data is missing or too old")]
    RevocationDataStale,
    /// Step 9: the invocation is not well formed.
    #[error("step 9: the invocation is malformed")]
    InvocationMalformed,
    /// Step 9: the invocation cites a different last grant than the chain has.
    #[error("step 9: the invocation cites a different chain")]
    ChainMismatch,
    /// Step 9: the invoker is not the last grant's audience.
    #[error("step 9: the invoker is not the audience of the last grant")]
    InvokerNotAudience,
    /// Step 9: the invocation names another recipient.
    #[error("step 9: the invocation is for another recipient")]
    WrongRecipient,
    /// Step 9: the invocation is bound to another session.
    #[error("step 9: the invocation is bound to another session")]
    WrongSession,
    /// Step 9: the invocation is too old or dated in the future.
    #[error("step 9: the invocation time is outside the accepted window")]
    InvocationStale,
    /// Step 9: the invocation signature does not verify.
    #[error("step 9: the invocation signature does not verify")]
    InvocationSignature,
    /// Step 9: the invocation's nonce has been seen.
    #[error("step 9: the invocation nonce was already used")]
    NonceReplayed,
    /// Step 10: a resource is not covered by the last grant.
    #[error("step 10: a resource is not covered by the last grant")]
    ResourceNotCovered,
    /// Step 10: the action is not allowed by the last grant.
    #[error("step 10: the action is not allowed by the last grant")]
    ActionNotGranted,
    /// Step 10: the declared effect class is above the last grant's ceiling.
    #[error("step 10: the effect class is above the last grant's ceiling")]
    EffectExceeded,
    /// Step 11: a limit would be exceeded.
    #[error("step 11: a limit is exhausted")]
    LimitExhausted,
    /// Step 11: a grant has a limit and the ledger cannot say what was used.
    #[error("step 11: usage is unknown for a grant that has a limit")]
    UsageUnknown,
}

impl Denial {
    /// The number of the verification step that refused, 1 to 11.
    pub fn step(&self) -> u8 {
        match self {
            Denial::ChainLimits | Denial::InvocationLimits => 1,
            Denial::Malformed | Denial::UnknownVersion | Denial::UnknownCaveat => 2,
            Denial::RootNotAuthoritative => 3,
            Denial::BrokenLink => 4,
            Denial::BadGrantSignature => 5,
            Denial::Broadened => 6,
            Denial::NotYetValid | Denial::Expired => 7,
            Denial::Revoked | Denial::RevocationDataStale => 8,
            Denial::InvocationMalformed
            | Denial::ChainMismatch
            | Denial::InvokerNotAudience
            | Denial::WrongRecipient
            | Denial::WrongSession
            | Denial::InvocationStale
            | Denial::InvocationSignature
            | Denial::NonceReplayed => 9,
            Denial::ResourceNotCovered | Denial::ActionNotGranted | Denial::EffectExceeded => 10,
            Denial::LimitExhausted | Denial::UsageUnknown => 11,
        }
    }

    /// The name of this variant, as the test vectors spell it.
    pub fn name(&self) -> &'static str {
        match self {
            Denial::ChainLimits => "ChainLimits",
            Denial::InvocationLimits => "InvocationLimits",
            Denial::Malformed => "Malformed",
            Denial::UnknownVersion => "UnknownVersion",
            Denial::UnknownCaveat => "UnknownCaveat",
            Denial::RootNotAuthoritative => "RootNotAuthoritative",
            Denial::BrokenLink => "BrokenLink",
            Denial::BadGrantSignature => "BadGrantSignature",
            Denial::Broadened => "Broadened",
            Denial::NotYetValid => "NotYetValid",
            Denial::Expired => "Expired",
            Denial::Revoked => "Revoked",
            Denial::RevocationDataStale => "RevocationDataStale",
            Denial::InvocationMalformed => "InvocationMalformed",
            Denial::ChainMismatch => "ChainMismatch",
            Denial::InvokerNotAudience => "InvokerNotAudience",
            Denial::WrongRecipient => "WrongRecipient",
            Denial::WrongSession => "WrongSession",
            Denial::InvocationStale => "InvocationStale",
            Denial::InvocationSignature => "InvocationSignature",
            Denial::NonceReplayed => "NonceReplayed",
            Denial::ResourceNotCovered => "ResourceNotCovered",
            Denial::ActionNotGranted => "ActionNotGranted",
            Denial::EffectExceeded => "EffectExceeded",
            Denial::LimitExhausted => "LimitExhausted",
            Denial::UsageUnknown => "UsageUnknown",
        }
    }
}

/// `true` if `spent + add` is over `limit`. An overflow counts as over.
pub(crate) fn over(spent: u64, add: u64, limit: Option<u64>) -> bool {
    match limit {
        None => false,
        Some(ceiling) => match spent.checked_add(add) {
            Some(total) => total > ceiling,
            None => true,
        },
    }
}

fn check_limits(chain: &Chain, invocation: &Invocation) -> Result<(), Denial> {
    if chain.is_empty() || chain.len() > MAX_CHAIN_DEPTH {
        return Err(Denial::ChainLimits);
    }
    let mut total: usize = 0;
    for grant in &chain.grants {
        if grant.body.check_bounds().is_err() {
            return Err(Denial::ChainLimits);
        }
        let size = match grant.encode() {
            Ok(bytes) => bytes.len(),
            Err(_) => return Err(Denial::ChainLimits),
        };
        if size > MAX_GRANT_BYTES {
            return Err(Denial::ChainLimits);
        }
        total = total.saturating_add(size);
    }
    if total > MAX_CHAIN_BYTES {
        return Err(Denial::ChainLimits);
    }
    if invocation.body.res.len() > MAX_INVOCATION_RESOURCES {
        return Err(Denial::InvocationLimits);
    }
    match invocation.encode() {
        Ok(bytes) if bytes.len() <= MAX_INVOCATION_BYTES => Ok(()),
        _ => Err(Denial::InvocationLimits),
    }
}

fn check_well_formed(chain: &Chain) -> Result<(), Denial> {
    for grant in &chain.grants {
        match grant.body.check_well_formed() {
            Ok(()) => {}
            Err(Malformation::UnknownVersion) => return Err(Denial::UnknownVersion),
            Err(Malformation::UnknownCaveat) => return Err(Denial::UnknownCaveat),
            Err(_) => return Err(Denial::Malformed),
        }
    }
    Ok(())
}

fn check_invocation(
    invocation: &Invocation,
    leaf: &Grant,
    leaf_id: &GrantId,
    ctx: &VerifyContext<'_>,
    skew: u64,
    ledger: &dyn UsageLedger,
) -> Result<(), Denial> {
    let body = &invocation.body;
    if !invocation.is_well_formed() {
        return Err(Denial::InvocationMalformed);
    }
    if body.leaf != *leaf_id {
        return Err(Denial::ChainMismatch);
    }
    if body.invoker != leaf.body.aud {
        return Err(Denial::InvokerNotAudience);
    }
    if body.recipient != ctx.recipient {
        return Err(Denial::WrongRecipient);
    }
    if body.session != ctx.session {
        return Err(Denial::WrongSession);
    }
    let too_new = body.time > ctx.now.saturating_add(skew);
    let too_old = ctx.now.saturating_sub(body.time) > ctx.max_invocation_age_secs;
    if too_new || too_old {
        return Err(Denial::InvocationStale);
    }
    if !invocation.verify_signature() {
        return Err(Denial::InvocationSignature);
    }
    if ledger.nonce_seen(&body.invoker, &body.nonce) {
        return Err(Denial::NonceReplayed);
    }
    Ok(())
}

/// Verifies an invocation against a chain: the eleven steps, in order.
///
/// Returns the [`Decision`] if every step passes, otherwise the first
/// [`Denial`]. `revocations` and `ledger` are read, never written; see
/// [`crate::MemoryLedger::charge`] for the caller's part. Step 8 asks
/// [`RevocationView::chain_revoked`].
pub fn verify(
    chain: &Chain,
    invocation: &Invocation,
    ctx: &VerifyContext<'_>,
    revocations: &dyn RevocationView,
    ledger: &dyn UsageLedger,
) -> Result<Decision, Denial> {
    let skew = ctx.skew_secs.min(MAX_CLOCK_SKEW_SECS);

    // Step 1: length and size limits.
    check_limits(chain, invocation)?;

    // Step 2: every grant is well formed, of a known version, with known caveats.
    check_well_formed(chain)?;

    let mut ids: Vec<GrantId> = Vec::with_capacity(chain.len());
    for grant in &chain.grants {
        match grant.id() {
            Ok(id) => ids.push(id),
            Err(_) => return Err(Denial::Malformed),
        }
    }
    let (root, leaf, leaf_id) = match (chain.root(), chain.leaf(), ids.last()) {
        (Some(root), Some(leaf), Some(leaf_id)) => (root, leaf, leaf_id),
        _ => return Err(Denial::ChainLimits),
    };

    // Step 3: the root's issuer is trusted and owns every resource touched.
    if !ctx.trusted_roots.contains(&root.body.iss) {
        return Err(Denial::RootNotAuthoritative);
    }
    if invocation
        .body
        .res
        .iter()
        .any(|resource| resource.owner != root.body.iss)
    {
        return Err(Denial::RootNotAuthoritative);
    }

    // Step 4: the chain is linked, by audience and by parent id.
    if root.body.parent.is_some() {
        return Err(Denial::BrokenLink);
    }
    for (index, pair) in chain.grants.windows(2).enumerate() {
        let linked =
            pair[1].body.iss == pair[0].body.aud && pair[1].body.parent == Some(ids[index]);
        if !linked {
            return Err(Denial::BrokenLink);
        }
    }

    // Step 5: every signature verifies against the issuer's key. Layer 2 has
    // no key succession yet, so the issuer's current key is `iss`.
    for grant in &chain.grants {
        if !grant.verify_signature() {
            return Err(Denial::BadGrantSignature);
        }
    }

    // Step 6: attenuation against the parent.
    for pair in chain.grants.windows(2) {
        if is_attenuation_of(&pair[1], &pair[0]).is_err() {
            return Err(Denial::Broadened);
        }
    }

    // Step 7: inside every validity window. Skew softens the start only.
    for grant in &chain.grants {
        if ctx.now.saturating_add(skew) < grant.body.nbf {
            return Err(Denial::NotYetValid);
        }
        if ctx.now >= grant.body.exp {
            return Err(Denial::Expired);
        }
    }

    // Step 8: nothing revoked, and the data is fresh enough for the effect class.
    if revocations.chain_revoked(chain, ids.as_slice()) {
        return Err(Denial::Revoked);
    }
    let bound = ctx.revocation_bounds.for_effect(invocation.body.effect);
    match revocations.data_age_secs(ctx.now) {
        Some(age) if age <= bound => {}
        _ => return Err(Denial::RevocationDataStale),
    }

    // Step 9: the invocation itself.
    check_invocation(invocation, leaf, leaf_id, ctx, skew, ledger)?;

    // Step 10: the operation is inside the last grant. Attenuation (step 6)
    // makes the last grant a subset of every grant before it.
    let leaf_body = &leaf.body;
    for resource in &invocation.body.res {
        if !leaf_body
            .res
            .iter()
            .any(|pattern| pattern.contains(resource))
        {
            return Err(Denial::ResourceNotCovered);
        }
    }
    if !leaf_body.act.contains(&invocation.body.act) {
        return Err(Denial::ActionNotGranted);
    }
    if invocation.body.effect > leaf_body.effect {
        return Err(Denial::EffectExceeded);
    }

    // Step 11: stateful limits, charged against every grant of the chain.
    for (grant, id) in chain.grants.iter().zip(ids.iter()) {
        let limits = &grant.body.limits;
        if !limits.any() {
            continue;
        }
        let spent = match ledger.spent(id, ctx.now) {
            Some(spent) => spent,
            None => return Err(Denial::UsageUnknown),
        };
        let exhausted = over(spent.calls, 1, limits.calls)
            || over(spent.cost, invocation.body.cost, limits.cost)
            || over(spent.bytes, invocation.body.bytes, limits.bytes)
            || over(spent.recent_calls, 1, limits.rate);
        if exhausted {
            return Err(Denial::LimitExhausted);
        }
    }

    let chain_limits: Vec<Limits> = chain.grants.iter().map(|g| g.body.limits).collect();
    let chain_expiries: Vec<u64> = chain.grants.iter().map(|g| g.body.exp).collect();
    Ok(Decision {
        chain_ids: ids,
        chain_limits,
        chain_expiries,
        nonce_horizon: ctx
            .now
            .saturating_add(skew)
            .saturating_add(ctx.max_invocation_age_secs),
        invoker: invocation.body.invoker,
        nonce: invocation.body.nonce,
        now: ctx.now,
        effect: invocation.body.effect,
        cost: invocation.body.cost,
        bytes: invocation.body.bytes,
    })
}

/// [`verify`] over encoded input: each grant and the invocation are decoded
/// first ([`Grant::decode`], [`Invocation::decode`]), so non-canonical bytes,
/// unknown fields and oversize input are refused here.
///
/// A decode failure is reported as it is met, chain first and then the
/// invocation, before any semantic step; an oversize or over-bound input is
/// `ChainLimits` or `InvocationLimits` (step 1) and anything else that fails
/// to decode is `Malformed` or `InvocationMalformed`.
pub fn verify_encoded(
    chain_bytes: &[Vec<u8>],
    invocation_bytes: &[u8],
    ctx: &VerifyContext<'_>,
    revocations: &dyn RevocationView,
    ledger: &dyn UsageLedger,
) -> Result<Decision, Denial> {
    if chain_bytes.is_empty() || chain_bytes.len() > MAX_CHAIN_DEPTH {
        return Err(Denial::ChainLimits);
    }
    let mut grants: Vec<Grant> = Vec::with_capacity(chain_bytes.len());
    for bytes in chain_bytes {
        match Grant::decode(bytes) {
            Ok(grant) => grants.push(grant),
            Err(DecodeError::TooLarge(_)) | Err(DecodeError::Bounds(_)) => {
                return Err(Denial::ChainLimits)
            }
            Err(_) => return Err(Denial::Malformed),
        }
    }
    let invocation = match Invocation::decode(invocation_bytes) {
        Ok(invocation) => invocation,
        Err(DecodeError::TooLarge(_)) | Err(DecodeError::Bounds(_)) => {
            return Err(Denial::InvocationLimits)
        }
        Err(_) => return Err(Denial::InvocationMalformed),
    };
    verify(&Chain::new(grants), &invocation, ctx, revocations, ledger)
}
