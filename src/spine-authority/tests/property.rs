//! Property-style tests with a seeded generator (no proptest): over many
//! random chains, some of them deliberately broadened by a random mutation,
//! nothing that verifies conveys an effect, resource, time or limit its root
//! did not convey, and an untouched narrowing chain always verifies.
//!
//! The resource oracle is semantic, not the crate's own rule: every name in a
//! small universe (all paths of depth at most 3 over three letters) is tested
//! against the patterns of the root and of the leaf.

mod common;

use common::*;
use ed25519_dalek::SigningKey;
use spine_authority::{
    verify, Action, Chain, Effect, Grant, GrantBody, GrantSpec, IssueError, LimitKind, Limits,
    MemoryLedger, ResourceName, ResourcePattern, RevocationSet, VerifyContext, GRANT_VERSION,
    INVOCATION_VERSION, MAX_CHAIN_DEPTH,
};

const ALPHABET: [&str; 3] = ["a", "b", "c"];
const ALL_ACTIONS: [Action; 4] = [Action::Read, Action::Invoke, Action::Publish, Action::Admin];
const EFFECTS: [Effect; 3] = [Effect::Read, Effect::Reversible, Effect::Irreversible];

fn rand_path(rng: &mut Rng, max_len: u64) -> Vec<String> {
    let n = rng.below(max_len + 1);
    (0..n)
        .map(|_| ALPHABET[rng.below(3) as usize].to_string())
        .collect()
}

fn universe(owner: &SigningKey) -> Vec<ResourceName> {
    let mut paths: Vec<Vec<String>> = vec![Vec::new()];
    let mut frontier: Vec<Vec<String>> = vec![Vec::new()];
    for _ in 0..3 {
        let mut next: Vec<Vec<String>> = Vec::new();
        for p in &frontier {
            for letter in ALPHABET {
                let mut q = p.clone();
                q.push(letter.to_string());
                next.push(q);
            }
        }
        paths.extend(next.iter().cloned());
        frontier = next;
    }
    paths
        .into_iter()
        .map(|path| ResourceName {
            owner: public(owner),
            path,
        })
        .collect()
}

fn contained(patterns: &[ResourcePattern], r: &ResourceName) -> bool {
    patterns.iter().any(|p| p.contains(r))
}

fn rand_acts(rng: &mut Rng, from: &[Action]) -> Vec<Action> {
    let mut out: Vec<Action> = from.iter().copied().filter(|_| rng.chance(2)).collect();
    if out.is_empty() {
        out.push(from[rng.below(from.len() as u64) as usize]);
    }
    out
}

fn rand_limit(rng: &mut Rng, parent: Option<u64>) -> Option<u64> {
    match parent {
        Some(p) => Some(1 + rng.below(p)),
        None => {
            if rng.chance(2) {
                Some(10 + rng.below(90))
            } else {
                None
            }
        }
    }
}

fn rand_root(rng: &mut Rng, owner: &SigningKey, aud: &SigningKey) -> GrantBody {
    let res: Vec<ResourcePattern> = (0..1 + rng.below(2))
        .map(|_| ResourcePattern {
            owner: public(owner),
            path: rand_path(rng, 2),
            prefix: !rng.chance(4),
        })
        .collect();
    let nbf = 100 + rng.below(300);
    GrantBody {
        version: GRANT_VERSION,
        iss: public(owner),
        aud: public(aud),
        parent: Some(AUTO),
        role: "principal".to_string(),
        res,
        act: rand_acts(rng, &ALL_ACTIONS),
        effect: EFFECTS[rng.below(3) as usize],
        delegable: 6,
        nbf,
        exp: nbf + 600 + rng.below(1_000),
        limits: Limits {
            calls: rand_limit(rng, None),
            cost: rand_limit(rng, None),
            bytes: rand_limit(rng, None),
            rate: rand_limit(rng, None),
        },
        caveats: Vec::new(),
        nonce: [rng.below(256) as u8; 16],
    }
}

/// A random narrowing of `parent`, issued by the parent's audience to `aud`.
/// The second value is true when no narrowing exists (the parent cannot
/// delegate), so the child is necessarily a broadening.
fn narrow(
    rng: &mut Rng,
    parent: &GrantBody,
    issuer: &SigningKey,
    aud: &SigningKey,
) -> (GrantBody, bool) {
    let res: Vec<ResourcePattern> = (0..1 + rng.below(2))
        .map(|_| {
            let p = &parent.res[rng.below(parent.res.len() as u64) as usize];
            if p.prefix {
                let mut path = p.path.clone();
                path.extend(rand_path(rng, 2));
                ResourcePattern {
                    owner: p.owner,
                    path,
                    prefix: rng.chance(2),
                }
            } else {
                p.clone()
            }
        })
        .collect();
    let effects: Vec<Effect> = EFFECTS
        .iter()
        .copied()
        .filter(|e| *e <= parent.effect)
        .collect();
    let delegable = if parent.delegable == 0 {
        0
    } else if rng.chance(3) {
        rng.below(u64::from(parent.delegable)) as u32
    } else {
        parent.delegable - 1
    };
    let nbf = parent.nbf + rng.below(50);
    let exp = parent.exp - rng.below(50);
    let body = GrantBody {
        version: GRANT_VERSION,
        iss: public(issuer),
        aud: public(aud),
        parent: Some(AUTO),
        role: "agent".to_string(),
        res,
        act: rand_acts(rng, &parent.act),
        effect: effects[rng.below(effects.len() as u64) as usize],
        delegable,
        nbf,
        exp,
        limits: Limits {
            calls: rand_limit(rng, parent.limits.calls),
            cost: rand_limit(rng, parent.limits.cost),
            bytes: rand_limit(rng, parent.limits.bytes),
            rate: rand_limit(rng, parent.limits.rate),
        },
        caveats: Vec::new(),
        nonce: [rng.below(256) as u8; 16],
    };
    (body, parent.delegable == 0)
}

/// Applies one random broadening to `b`.
fn widen(
    rng: &mut Rng,
    b: &mut GrantBody,
    parent: &GrantBody,
    owner: &SigningKey,
    other: &SigningKey,
) {
    match rng.below(10) {
        0 => b.res.push(ResourcePattern {
            owner: public(owner),
            path: Vec::new(),
            prefix: true,
        }),
        1 => b.act = ALL_ACTIONS.to_vec(),
        2 => b.effect = Effect::Irreversible,
        3 => b.delegable = parent.delegable + rng.below(3) as u32,
        4 => b.nbf = parent.nbf.saturating_sub(1 + rng.below(50)),
        5 => b.exp = parent.exp + 1 + rng.below(50),
        6 => b.limits.calls = None,
        7 => b.limits.cost = parent.limits.cost.map(|p| p + 1 + rng.below(5)),
        8 => {
            b.res = vec![ResourcePattern {
                owner: public(other),
                path: Vec::new(),
                prefix: true,
            }]
        }
        _ => b.limits.rate = parent.limits.rate.map(|p| p + 1),
    }
}

#[test]
fn nothing_that_verifies_conveys_more_than_its_root() {
    let keys: Vec<SigningKey> = (1u8..=7).map(key).collect();
    let stranger = key(60);
    let recipient = key(61);
    let session = [0x33u8; 32];
    let names = universe(&keys[0]);
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);

    let mut accepted = 0usize;
    let mut rejected = 0usize;
    let mut untouched_checked = 0usize;
    let mut denials: std::collections::BTreeSet<&'static str> = std::collections::BTreeSet::new();

    for round in 0..300u32 {
        let len = 1 + rng.below(4) as usize;
        let mut bodies: Vec<GrantBody> = vec![rand_root(&mut rng, &keys[0], &keys[1])];
        let mut mutated = false;
        let mut forced_bad = false;
        for i in 1..len {
            let (mut child, forced) = narrow(&mut rng, &bodies[i - 1], &keys[i], &keys[i + 1]);
            forced_bad |= forced;
            if rng.chance(2) {
                let parent = bodies[i - 1].clone();
                widen(&mut rng, &mut child, &parent, &keys[0], &stranger);
                mutated = true;
            }
            bodies.push(child);
        }
        let signers: Vec<&SigningKey> = keys.iter().take(len).collect();
        let grants = sign_chain(&bodies, &signers);
        let root = grants[0].body.clone();
        let leaf = grants[len - 1].body.clone();

        // The invocation: inside the leaf when the chain is untouched, and
        // otherwise as likely to be outside it.
        let untouched = !(mutated || forced_bad);
        let inside = untouched || rng.chance(2);
        let (resource, action, effect, now) = if inside {
            let pattern = &leaf.res[rng.below(leaf.res.len() as u64) as usize];
            let mut path = pattern.path.clone();
            if pattern.prefix {
                path.extend(rand_path(&mut rng, 2));
            }
            let effects: Vec<Effect> = EFFECTS
                .iter()
                .copied()
                .filter(|e| *e <= leaf.effect)
                .collect();
            (
                ResourceName {
                    owner: pattern.owner,
                    path,
                },
                leaf.act[rng.below(leaf.act.len() as u64) as usize],
                effects[rng.below(effects.len() as u64) as usize],
                leaf.nbf + rng.below(leaf.exp.saturating_sub(leaf.nbf).max(1)),
            )
        } else {
            (
                ResourceName {
                    owner: if rng.chance(4) {
                        public(&stranger)
                    } else {
                        public(&keys[0])
                    },
                    path: rand_path(&mut rng, 3),
                },
                ALL_ACTIONS[rng.below(4) as usize],
                EFFECTS[rng.below(3) as usize],
                root.nbf.saturating_sub(10) + rng.below(root.exp - root.nbf + 20),
            )
        };

        let invoker = &keys[len];
        let leaf_id = grants[len - 1].id().unwrap();
        let invocation = spine_authority::Invocation::sign(
            invoker,
            spine_authority::InvocationBody {
                version: INVOCATION_VERSION,
                invoker: public(invoker),
                recipient: public(&recipient),
                session,
                nonce: [round as u8; 16],
                time: now,
                res: vec![resource.clone()],
                act: action,
                effect,
                op: [0u8; 32],
                leaf: leaf_id,
                cost: rng.below(2),
                bytes: rng.below(2),
            },
        )
        .unwrap();

        let roots = vec![public(&keys[0])];
        let ctx = VerifyContext::new(now, public(&recipient), session, &roots);
        let revocations = RevocationSet::current(now);
        let ledger = MemoryLedger::new();
        let result = verify(
            &Chain::new(grants.clone()),
            &invocation,
            &ctx,
            &revocations,
            &ledger,
        );

        match result {
            Err(denial) => {
                rejected += 1;
                denials.insert(denial.name());
                assert!(
                    !(untouched && inside),
                    "round {round}: an untouched narrowing chain was refused: {denial:?}"
                );
            }
            Ok(decision) => {
                accepted += 1;
                if untouched {
                    untouched_checked += 1;
                }
                assert_eq!(decision.chain_ids.len(), len);

                // The invocation is inside what the ROOT conveyed.
                assert!(contained(&root.res, &resource), "round {round}: resource");
                assert!(root.act.contains(&action), "round {round}: action");
                assert!(effect <= root.effect, "round {round}: effect");
                assert!(root.nbf <= now && now < root.exp, "round {round}: time");
                assert_eq!(resource.owner, root.iss, "round {round}: owner");

                // Everything the leaf conveys is inside what the root conveyed.
                for name in &names {
                    if contained(&leaf.res, name) {
                        assert!(contained(&root.res, name), "round {round}: leaf resource");
                    }
                }
                for a in &leaf.act {
                    assert!(root.act.contains(a), "round {round}: leaf action");
                }
                assert!(leaf.effect <= root.effect, "round {round}: leaf effect");
                assert!(
                    leaf.nbf >= root.nbf && leaf.exp <= root.exp,
                    "round {round}: window"
                );
                for kind in LimitKind::ALL {
                    if let Some(ceiling) = root.limits.get(kind) {
                        match leaf.limits.get(kind) {
                            Some(l) => assert!(l <= ceiling, "round {round}: {kind:?} raised"),
                            None => panic!("round {round}: {kind:?} dropped"),
                        }
                    }
                }

                // And link by link: delegable falls, windows and limits nest.
                for pair in grants.windows(2) {
                    let (p, c) = (&pair[0].body, &pair[1].body);
                    assert!(c.delegable < p.delegable, "round {round}: delegable");
                    assert!(c.nbf >= p.nbf && c.exp <= p.exp, "round {round}: nesting");
                    assert!(c.effect <= p.effect, "round {round}: effect nesting");
                    assert!(
                        c.act.iter().all(|a| p.act.contains(a)),
                        "round {round}: acts"
                    );
                    for name in &names {
                        if contained(&c.res, name) {
                            assert!(contained(&p.res, name), "round {round}: nesting");
                        }
                    }
                }
                assert!(len <= MAX_CHAIN_DEPTH);
            }
        }
    }

    // The run must not be vacuous in either direction.
    assert!(accepted >= 20, "only {accepted} chains verified");
    assert!(rejected >= 20, "only {rejected} chains were refused");
    assert!(
        untouched_checked >= 10,
        "only {untouched_checked} untouched chains"
    );
    // Some random mutations are harmless (a no-op widening); what matters is
    // that every accepted chain passed the oracle above.
    // Several different steps did the refusing.
    assert!(denials.len() >= 4, "refusals came from only {denials:?}");
}

fn spec_of(b: &GrantBody) -> GrantSpec {
    GrantSpec {
        aud: b.aud,
        role: b.role.clone(),
        res: b.res.clone(),
        act: b.act.clone(),
        effect: b.effect,
        delegable: b.delegable,
        nbf: b.nbf,
        exp: b.exp,
        limits: b.limits,
        caveats: Vec::new(),
        nonce: b.nonce,
    }
}

#[test]
fn delegate_never_signs_a_grant_broader_than_its_parent() {
    let keys: Vec<SigningKey> = (1u8..=3).map(key).collect();
    let other = key(62);
    let names = universe(&keys[0]);
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let mut signed = 0usize;
    let mut refused = 0usize;

    for _ in 0..12 {
        let root_body = rand_root(&mut rng, &keys[0], &keys[1]);
        let parent = Grant::issue_root(&keys[0], spec_of(&root_body)).unwrap();
        for _ in 0..80 {
            // Half the attempts start from a real narrowing, half are
            // arbitrary; both are then often broadened.
            let (mut body, _) = narrow(&mut rng, &parent.body, &keys[1], &keys[2]);
            if rng.chance(2) {
                let p = parent.body.clone();
                widen(&mut rng, &mut body, &p, &keys[0], &other);
            }
            if rng.chance(4) {
                body.res = vec![ResourcePattern {
                    owner: public(&keys[0]),
                    path: rand_path(&mut rng, 3),
                    prefix: rng.chance(2),
                }];
            }
            match Grant::delegate(&keys[1], &parent, spec_of(&body)) {
                Ok(child) => {
                    signed += 1;
                    let (p, c) = (&parent.body, &child.body);
                    for name in &names {
                        if contained(&c.res, name) {
                            assert!(contained(&p.res, name), "resource");
                        }
                    }
                    assert!(c.act.iter().all(|a| p.act.contains(a)));
                    assert!(c.effect <= p.effect);
                    assert!(c.delegable < p.delegable);
                    assert!(c.nbf >= p.nbf && c.exp <= p.exp);
                    for kind in LimitKind::ALL {
                        if let Some(ceiling) = p.limits.get(kind) {
                            assert!(c.limits.get(kind).is_some_and(|l| l <= ceiling));
                        }
                    }
                    assert_eq!(c.parent, Some(parent.id().unwrap()));
                    assert!(child.verify_signature());
                }
                Err(IssueError::Broadens(_)) | Err(IssueError::Malformed(_)) => refused += 1,
                Err(other_error) => panic!("unexpected error {other_error:?}"),
            }
        }
    }
    assert!(signed >= 50, "only {signed} grants were signed");
    assert!(refused >= 50, "only {refused} attempts were refused");
}
