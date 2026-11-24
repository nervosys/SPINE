//! Attenuation: a child grant may only narrow its parent.
//!
//! The rules, from `docs/spec/03-authority.md`, checked in this order:
//!
//! 1. the child's issuer is the parent's audience;
//! 2. every child resource pattern is covered by some parent pattern
//!    ([`crate::ResourcePattern::covered_by`], segment-wise);
//! 3. the child's actions are a subset of the parent's;
//! 4. the child's effect ceiling is no higher;
//! 5. the child's `delegable` is strictly lower;
//! 6. the child's window lies inside the parent's: `nbf` no earlier, `exp` no
//!    later;
//! 7. wherever the parent sets a limit the child sets one too, and it is no
//!    higher;
//! 8. every parent caveat appears in the child (the child may add more).
//!
//! The first rule that fails is reported.

use crate::grant::{Grant, GrantBody, LimitKind};

/// What a child grant broadened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Broadening {
    /// The child's issuer is not the parent's audience.
    #[error("issuer is not the parent's audience")]
    Audience,
    /// A child resource pattern is not covered by any parent pattern.
    #[error("a resource pattern is not covered by the parent")]
    Resource,
    /// The child allows an action the parent does not.
    #[error("an action is not allowed by the parent")]
    Action,
    /// The child's effect ceiling is above the parent's.
    #[error("effect ceiling is above the parent's")]
    Effect,
    /// The child's `delegable` is not strictly lower than the parent's.
    #[error("delegable is not strictly lower than the parent's")]
    Delegable,
    /// The child is valid from before the parent is.
    #[error("valid from before the parent")]
    NotBefore,
    /// The child is valid until after the parent expires.
    #[error("valid until after the parent")]
    Expiry,
    /// The child's limit of this kind is higher than the parent's.
    #[error("a limit is higher than the parent's")]
    LimitRaised(LimitKind),
    /// The parent sets a limit of this kind and the child does not.
    #[error("a limit the parent sets is missing")]
    LimitRemoved(LimitKind),
    /// A caveat of the parent is missing from the child.
    #[error("a caveat of the parent is missing")]
    CaveatDropped,
}

/// [`is_attenuation_of`] on bodies, for callers that have not signed yet.
pub fn body_is_attenuation_of(child: &GrantBody, parent: &GrantBody) -> Result<(), Broadening> {
    if child.iss != parent.aud {
        return Err(Broadening::Audience);
    }
    for pattern in &child.res {
        if !parent.res.iter().any(|outer| pattern.covered_by(outer)) {
            return Err(Broadening::Resource);
        }
    }
    for action in &child.act {
        if !parent.act.contains(action) {
            return Err(Broadening::Action);
        }
    }
    if child.effect > parent.effect {
        return Err(Broadening::Effect);
    }
    if child.delegable >= parent.delegable {
        return Err(Broadening::Delegable);
    }
    if child.nbf < parent.nbf {
        return Err(Broadening::NotBefore);
    }
    if child.exp > parent.exp {
        return Err(Broadening::Expiry);
    }
    for kind in LimitKind::ALL {
        match (parent.limits.get(kind), child.limits.get(kind)) {
            (Some(_), None) => return Err(Broadening::LimitRemoved(kind)),
            (Some(outer), Some(inner)) if inner > outer => {
                return Err(Broadening::LimitRaised(kind));
            }
            _ => {}
        }
    }
    for caveat in &parent.caveats {
        if !child.caveats.contains(caveat) {
            return Err(Broadening::CaveatDropped);
        }
    }
    Ok(())
}

/// Whether `child` is no broader than `parent` in every field, naming the
/// first thing it broadened if not. Signatures and the `parent` reference are
/// not looked at; those are verification steps 4 and 5.
pub fn is_attenuation_of(child: &Grant, parent: &Grant) -> Result<(), Broadening> {
    body_is_attenuation_of(&child.body, &parent.body)
}
