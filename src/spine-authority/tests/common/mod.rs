//! Shared fixtures for the integration tests. Not every test file uses every
//! helper.
#![allow(dead_code)]

use ed25519_dalek::SigningKey;
use spine_authority::{
    Action, Caveat, Chain, Effect, Grant, GrantBody, GrantId, Invocation, InvocationBody, Limits,
    MemoryLedger, PublicKey, ResourceName, ResourcePattern, RevocationSet, VerifyContext,
    GRANT_VERSION, INVOCATION_VERSION,
};
use spine_name::SpineUri;

/// The verifier's clock in the standard scenario.
pub const NOW: u64 = 5_000;

/// Placeholder parent: `sign_chain` replaces it with the real id of the
/// previous grant. Any other value is kept as it is.
pub const AUTO: GrantId = [0u8; 32];

pub fn key(n: u8) -> SigningKey {
    SigningKey::from_bytes(&[n; 32])
}

pub fn public(k: &SigningKey) -> PublicKey {
    k.verifying_key().to_bytes()
}

pub fn uri(owner: &SigningKey, path: &str) -> SpineUri {
    SpineUri::did(public(owner)).with_path(path)
}

pub fn prefix(owner: &SigningKey, path: &str) -> ResourcePattern {
    ResourcePattern::prefix(&uri(owner, path)).unwrap()
}

pub fn exact(owner: &SigningKey, path: &str) -> ResourcePattern {
    ResourcePattern::exact(&uri(owner, path)).unwrap()
}

pub fn name(owner: &SigningKey, path: &str) -> ResourceName {
    ResourceName::from_uri(&uri(owner, path)).unwrap()
}

#[allow(clippy::too_many_arguments)]
pub fn body(
    iss: &SigningKey,
    aud: &SigningKey,
    role: &str,
    res: Vec<ResourcePattern>,
    act: Vec<Action>,
    effect: Effect,
    delegable: u32,
    window: (u64, u64),
    limits: Limits,
    nonce: u8,
) -> GrantBody {
    GrantBody {
        version: GRANT_VERSION,
        iss: public(iss),
        aud: public(aud),
        parent: Some(AUTO),
        role: role.to_string(),
        res,
        act,
        effect,
        delegable,
        nbf: window.0,
        exp: window.1,
        limits,
        caveats: Vec::new(),
        nonce: [nonce; 16],
    }
}

/// Signs `bodies[i]` with `keys[i]`, filling in `AUTO` parents. The first
/// body's parent is made `None` if it is `AUTO`.
pub fn sign_chain(bodies: &[GrantBody], keys: &[&SigningKey]) -> Vec<Grant> {
    let mut grants: Vec<Grant> = Vec::new();
    for (i, b) in bodies.iter().enumerate() {
        let mut b = b.clone();
        if b.parent == Some(AUTO) {
            b.parent = if i == 0 {
                None
            } else {
                Some(grants[i - 1].id().unwrap())
            };
        }
        grants.push(Grant::sign_unchecked(keys[i], b).unwrap());
    }
    grants
}

/// The standard scenario: owner `k0` delegates through `k1` and `k2` to `k3`,
/// who invokes `invoke` on `k0:/data/reports/q3` with effect `reversible`.
pub struct World {
    pub keys: Vec<SigningKey>,
    pub bodies: Vec<GrantBody>,
    pub inv: InvocationBody,
    pub recipient: SigningKey,
    pub session: [u8; 32],
}

impl World {
    pub fn new() -> World {
        let keys: Vec<SigningKey> = (1u8..=4).map(key).collect();
        let k = |i: usize| &keys[i];
        let limits = |calls, cost, bytes, rate| Limits {
            calls: Some(calls),
            cost: Some(cost),
            bytes: Some(bytes),
            rate: Some(rate),
        };
        let bodies = vec![
            body(
                k(0),
                k(1),
                "principal",
                vec![prefix(k(0), "/data")],
                vec![Action::Read, Action::Invoke, Action::Publish],
                Effect::Irreversible,
                3,
                (1_000, 10_000),
                limits(100, 1_000, 1_000_000, 10),
                1,
            ),
            body(
                k(1),
                k(2),
                "operator",
                vec![prefix(k(0), "/data/reports")],
                vec![Action::Read, Action::Invoke],
                Effect::Reversible,
                2,
                (1_000, 9_000),
                limits(50, 500, 500_000, 5),
                2,
            ),
            body(
                k(2),
                k(3),
                "agent",
                vec![exact(k(0), "/data/reports/q3")],
                vec![Action::Invoke],
                Effect::Reversible,
                0,
                (2_000, 8_000),
                limits(10, 100, 10_000, 2),
                3,
            ),
        ];
        let recipient = key(9);
        let session = [0x55u8; 32];
        let inv = InvocationBody {
            version: INVOCATION_VERSION,
            invoker: public(k(3)),
            recipient: public(&recipient),
            session,
            nonce: [7u8; 16],
            time: NOW,
            res: vec![name(k(0), "/data/reports/q3")],
            act: Action::Invoke,
            effect: Effect::Reversible,
            op: [0xabu8; 32],
            leaf: AUTO,
            cost: 1,
            bytes: 10,
        };
        World {
            keys,
            bodies,
            inv,
            recipient,
            session,
        }
    }

    pub fn grants(&self) -> Vec<Grant> {
        let refs: Vec<&SigningKey> = self.keys.iter().take(self.bodies.len()).collect();
        sign_chain(&self.bodies, &refs)
    }

    pub fn chain(&self) -> Chain {
        Chain::new(self.grants())
    }

    /// The invocation, signed by `keys[3]`, citing the last of `grants` unless
    /// `inv.leaf` was set to something else.
    pub fn invocation(&self, grants: &[Grant]) -> Invocation {
        self.invocation_by(grants, &self.keys[3])
    }

    pub fn invocation_by(&self, grants: &[Grant], signer: &SigningKey) -> Invocation {
        let mut b = self.inv.clone();
        if b.leaf == AUTO {
            b.leaf = grants.last().unwrap().id().unwrap();
        }
        Invocation::sign(signer, b).unwrap()
    }

    /// The invocation body with its time changed.
    pub fn clone_inv_at(&self, time: u64) -> InvocationBody {
        let mut b = self.inv.clone();
        b.time = time;
        b
    }

    pub fn roots(&self) -> Vec<PublicKey> {
        vec![public(&self.keys[0])]
    }

    pub fn revocations(&self) -> RevocationSet {
        RevocationSet::current(NOW)
    }

    pub fn ledger(&self) -> MemoryLedger {
        MemoryLedger::new()
    }
}

pub fn ctx<'a>(w: &World, roots: &'a [PublicKey]) -> VerifyContext<'a> {
    VerifyContext::new(NOW, public(&w.recipient), w.session, roots)
}

pub fn caveat(name: &str) -> Caveat {
    Caveat {
        name: name.to_string(),
        arg: vec![1, 2, 3],
    }
}

/// A small deterministic generator (xorshift64*), so the property test needs
/// no dependency and gives the same run everywhere.
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..n` (`n` must be positive).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    pub fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
}
