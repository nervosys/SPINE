//! One negative test for every verification step and every denial variant:
//! each builds an otherwise valid chain that fails exactly that step and
//! asserts the exact variant. The standard scenario is in `common::World`.

mod common;

use common::*;
use spine_authority::{
    verify, verify_encoded, Action, Chain, Decision, Denial, Effect, Grant, GrantId, Invocation,
    Limits, MemoryLedger, NoLedger, NoRevocationData, PublicKey, RevocationBounds, RevocationSet,
    RevocationView, Spent, UsageLedger, VerifyContext,
};

/// A ledger that never saw a nonce and cannot say what was spent.
struct UnknownLedger;

impl UsageLedger for UnknownLedger {
    fn nonce_seen(&self, _invoker: &PublicKey, _nonce: &[u8; 16]) -> bool {
        false
    }

    fn spent(&self, _id: &GrantId, _now: u64) -> Option<Spent> {
        None
    }
}

fn run_full(
    grants: &[Grant],
    inv: &Invocation,
    c: &VerifyContext<'_>,
    rev: &dyn RevocationView,
    ledger: &dyn UsageLedger,
) -> Result<Decision, Denial> {
    verify(&Chain::new(grants.to_vec()), inv, c, rev, ledger)
}

/// Verifies the world's own chain and invocation with the default policy.
fn run(w: &World, grants: &[Grant]) -> Result<Decision, Denial> {
    let inv = w.invocation(grants);
    let roots = w.roots();
    let c = ctx(w, &roots);
    run_full(grants, &inv, &c, &w.revocations(), &w.ledger())
}

fn expect(w: &World, grants: &[Grant], denial: Denial) {
    assert_eq!(run(w, grants), Err(denial));
    assert_eq!(denial.step(), run(w, grants).unwrap_err().step());
}

// ---------------------------------------------------------------------------
// the accepted case
// ---------------------------------------------------------------------------

#[test]
fn the_standard_scenario_verifies() {
    let w = World::new();
    let grants = w.grants();
    let decision = run(&w, &grants).expect("the standard scenario must verify");
    assert_eq!(decision.chain_ids.len(), 3);
    for (id, grant) in decision.chain_ids.iter().zip(grants.iter()) {
        assert_eq!(*id, grant.id().unwrap());
    }
    assert_eq!(decision.invoker, public(&w.keys[3]));
    assert_eq!(decision.effect, Effect::Reversible);
    assert_eq!(decision.cost, 1);
    assert_eq!(decision.bytes, 10);
    assert_eq!(decision.now, NOW);
}

#[test]
fn the_same_chain_verifies_when_issued_through_the_builders() {
    use spine_authority::GrantSpec;
    let w = World::new();
    let k = &w.keys;
    let spec =
        |aud: &ed25519_dalek::SigningKey, role: &str, b: &spine_authority::GrantBody| GrantSpec {
            aud: public(aud),
            role: role.to_string(),
            res: b.res.clone(),
            act: b.act.clone(),
            effect: b.effect,
            delegable: b.delegable,
            nbf: b.nbf,
            exp: b.exp,
            limits: b.limits,
            caveats: Vec::new(),
            nonce: b.nonce,
        };
    let g1 = Grant::issue_root(&k[0], spec(&k[1], "principal", &w.bodies[0])).unwrap();
    let g2 = Grant::delegate(&k[1], &g1, spec(&k[2], "operator", &w.bodies[1])).unwrap();
    let g3 = Grant::delegate(&k[2], &g2, spec(&k[3], "agent", &w.bodies[2])).unwrap();
    // The builders and the hand-built chain agree on everything but nothing
    // is random here, so they are byte-identical.
    assert_eq!(vec![g1.clone(), g2.clone(), g3.clone()], w.grants());
    assert!(run(&w, &[g1, g2, g3]).is_ok());
}

#[test]
fn a_decision_charged_to_the_ledger_makes_the_next_use_a_replay() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let c = ctx(&w, &roots);
    let rev = w.revocations();
    let mut ledger = MemoryLedger::new();
    let decision = run_full(&grants, &inv, &c, &rev, &ledger).unwrap();
    ledger.charge(&decision).unwrap();
    assert_eq!(
        run_full(&grants, &inv, &c, &rev, &ledger),
        Err(Denial::NonceReplayed)
    );
}

#[test]
fn the_fail_closed_defaults_refuse() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let c = ctx(&w, &roots);
    assert_eq!(
        run_full(&grants, &inv, &c, &NoRevocationData, &w.ledger()),
        Err(Denial::RevocationDataStale)
    );
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &NoLedger),
        Err(Denial::NonceReplayed)
    );
}

#[test]
fn the_first_failing_step_is_the_one_reported() {
    // Step 2 (unknown version) and step 5 (bad signature) both fail: step 2 wins.
    let mut w = World::new();
    w.bodies[1].version = 2;
    let mut grants = w.grants();
    grants[2].signature[0] ^= 1;
    expect(&w, &grants, Denial::UnknownVersion);
}

// ---------------------------------------------------------------------------
// step 1: limits
// ---------------------------------------------------------------------------

#[test]
fn step_1_an_empty_chain() {
    let mut w = World::new();
    w.inv.leaf = [1u8; 32];
    let grants: Vec<Grant> = Vec::new();
    let inv = w.invocation_by(&grants, &w.keys[3]);
    let roots = w.roots();
    let c = ctx(&w, &roots);
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &w.ledger()),
        Err(Denial::ChainLimits)
    );
}

#[test]
fn step_1_a_chain_longer_than_the_limit() {
    let mut w = World::new();
    let keys: Vec<ed25519_dalek::SigningKey> = (1u8..=10).map(key).collect();
    let mut bodies = Vec::new();
    for i in 0..9usize {
        bodies.push(body(
            &keys[i],
            &keys[i + 1],
            "agent",
            vec![prefix(&keys[0], "/data")],
            vec![Action::Invoke],
            Effect::Reversible,
            20 - i as u32,
            (1_000, 10_000),
            Limits::default(),
            i as u8 + 1,
        ));
    }
    let refs: Vec<&ed25519_dalek::SigningKey> = keys.iter().take(9).collect();
    let grants = sign_chain(&bodies, &refs);
    assert_eq!(grants.len(), spine_authority::MAX_CHAIN_DEPTH + 1);
    w.inv.invoker = public(&keys[9]);
    let inv = w.invocation_by(&grants, &keys[9]);
    let roots = w.roots();
    let c = ctx(&w, &roots);
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &w.ledger()),
        Err(Denial::ChainLimits)
    );
}

#[test]
fn step_1_a_grant_over_a_structural_bound() {
    let mut w = World::new();
    w.bodies[2].res = (0..17)
        .map(|i| exact(&w.keys[0], &format!("/data/reports/q{i}")))
        .collect();
    expect(&w, &w.grants(), Denial::ChainLimits);

    let mut w = World::new();
    w.bodies[2].res = vec![spine_authority::ResourcePattern {
        owner: public(&w.keys[0]),
        path: vec!["a".repeat(129)],
        prefix: false,
    }];
    expect(&w, &w.grants(), Denial::ChainLimits);

    let mut w = World::new();
    w.bodies[2].caveats = vec![spine_authority::Caveat {
        name: "x".to_string(),
        arg: vec![0u8; 257],
    }];
    expect(&w, &w.grants(), Denial::ChainLimits);
}

#[test]
fn step_1_an_invocation_over_the_resource_limit() {
    let mut w = World::new();
    w.inv.res = (0..17)
        .map(|i| name(&w.keys[0], &format!("/data/reports/q{i}")))
        .collect();
    expect(&w, &w.grants(), Denial::InvocationLimits);
}

// ---------------------------------------------------------------------------
// step 2: parse, version, caveats
// ---------------------------------------------------------------------------

#[test]
fn step_2_an_unknown_version() {
    let mut w = World::new();
    w.bodies[1].version = 2;
    expect(&w, &w.grants(), Denial::UnknownVersion);
}

#[test]
fn step_2_an_unrecognised_caveat() {
    let mut w = World::new();
    w.bodies[2].caveats = vec![caveat("needs-human")];
    expect(&w, &w.grants(), Denial::UnknownCaveat);
}

fn break_grant(label: &str, b: &mut spine_authority::GrantBody) {
    match label {
        "empty actions" => b.act.clear(),
        "unsorted actions" => b.act = vec![Action::Invoke, Action::Read],
        "duplicate actions" => b.act = vec![Action::Invoke, Action::Invoke],
        "empty resources" => b.res.clear(),
        "empty window" => b.nbf = b.exp,
        "unknown role" => b.role = "root".to_string(),
        "empty path segment" => b.res[0].path = vec!["".to_string()],
        "dotted path segment" => b.res[0].path = vec!["..".to_string()],
        _ => panic!("unknown case {label}"),
    }
}

#[test]
fn step_2_a_malformed_grant() {
    let labels = [
        "empty actions",
        "unsorted actions",
        "duplicate actions",
        "empty resources",
        "empty window",
        "unknown role",
        "empty path segment",
        "dotted path segment",
    ];
    for label in labels {
        let mut w = World::new();
        break_grant(label, &mut w.bodies[2]);
        let grants = w.grants();
        assert_eq!(run(&w, &grants), Err(Denial::Malformed), "{label}");
    }
}

// ---------------------------------------------------------------------------
// step 3: the root owns the resources
// ---------------------------------------------------------------------------

#[test]
fn step_3_a_root_this_verifier_does_not_trust() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let other: Vec<PublicKey> = vec![public(&key(30))];
    let c = ctx(&w, &other);
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &w.ledger()),
        Err(Denial::RootNotAuthoritative)
    );
    let none: Vec<PublicKey> = Vec::new();
    let c = ctx(&w, &none);
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &w.ledger()),
        Err(Denial::RootNotAuthoritative)
    );
}

#[test]
fn step_3_a_resource_the_root_does_not_own() {
    let mut w = World::new();
    // The root grant is genuine but the operation touches k2's name.
    w.inv.res = vec![name(&w.keys[1], "/data/reports/q3")];
    expect(&w, &w.grants(), Denial::RootNotAuthoritative);

    // One owned and one not: every resource must be owned.
    let mut w = World::new();
    w.inv.res = vec![
        name(&w.keys[0], "/data/reports/q3"),
        name(&w.keys[1], "/data/reports/q3"),
    ];
    expect(&w, &w.grants(), Denial::RootNotAuthoritative);
}

// ---------------------------------------------------------------------------
// step 4: links
// ---------------------------------------------------------------------------

#[test]
fn step_4_a_parent_that_is_not_the_previous_grant() {
    let mut w = World::new();
    w.bodies[1].parent = Some([9u8; 32]);
    expect(&w, &w.grants(), Denial::BrokenLink);
}

#[test]
fn step_4_an_issuer_that_is_not_the_previous_audience() {
    let w = World::new();
    let stranger = key(20);
    let mut bodies = w.bodies.clone();
    bodies[2].iss = public(&stranger);
    let grants = sign_chain(&bodies, &[&w.keys[0], &w.keys[1], &stranger]);
    expect(&w, &grants, Denial::BrokenLink);
}

#[test]
fn step_4_a_root_that_has_a_parent() {
    let mut w = World::new();
    w.bodies[0].parent = Some([5u8; 32]);
    expect(&w, &w.grants(), Denial::BrokenLink);
}

#[test]
fn step_4_grants_in_the_wrong_order() {
    let w = World::new();
    let mut grants = w.grants();
    grants.swap(1, 2);
    expect(&w, &grants, Denial::BrokenLink);
}

// ---------------------------------------------------------------------------
// step 5: signatures
// ---------------------------------------------------------------------------

#[test]
fn step_5_a_damaged_signature() {
    let w = World::new();
    let mut grants = w.grants();
    grants[1].signature[10] ^= 0x01;
    // The next grant's parent id is that of the undamaged grant, so keep the
    // chain linked by re-deriving the child from the damaged one.
    let mut bodies = w.bodies.clone();
    bodies[2].parent = Some(grants[1].id().unwrap());
    grants[2] = Grant::sign_unchecked(&w.keys[2], bodies[2].clone()).unwrap();
    expect(&w, &grants, Denial::BadGrantSignature);
}

#[test]
fn step_5_a_body_changed_after_signing() {
    let w = World::new();
    let mut grants = w.grants();
    grants[2].body.nonce = [0xeeu8; 16];
    expect(&w, &grants, Denial::BadGrantSignature);
}

#[test]
fn step_5_a_signature_by_another_key() {
    let w = World::new();
    let impostor = key(21);
    let grants = sign_chain(&w.bodies, &[&w.keys[0], &impostor, &w.keys[2]]);
    expect(&w, &grants, Denial::BadGrantSignature);
}

// ---------------------------------------------------------------------------
// step 6: attenuation
// ---------------------------------------------------------------------------

fn widen_grant(label: &str, b: &mut spine_authority::GrantBody, w: &World) {
    match label {
        "wider resource" => b.res = vec![prefix(&w.keys[0], "/data")],
        "other owner" => b.res = vec![exact(&w.keys[1], "/data/reports/q3")],
        "sibling by raw prefix" => b.res = vec![exact(&w.keys[0], "/data/reportsX")],
        "extra action" => b.act = vec![Action::Read, Action::Invoke, Action::Publish],
        "higher effect" => b.effect = Effect::Irreversible,
        "delegable not lower" => b.delegable = 2,
        "earlier start" => b.nbf = 500,
        "later end" => b.exp = 9_500,
        "higher calls" => b.limits.calls = Some(60),
        "higher cost" => b.limits.cost = Some(501),
        "higher bytes" => b.limits.bytes = Some(500_001),
        "higher rate" => b.limits.rate = Some(6),
        "no calls limit" => b.limits.calls = None,
        "no rate limit" => b.limits.rate = None,
        _ => panic!("unknown case {label}"),
    }
}

#[test]
fn step_6_each_way_of_broadening_is_refused() {
    let labels = [
        "wider resource",
        "other owner",
        "sibling by raw prefix",
        "extra action",
        "higher effect",
        "delegable not lower",
        "earlier start",
        "later end",
        "higher calls",
        "higher cost",
        "higher bytes",
        "higher rate",
        "no calls limit",
        "no rate limit",
    ];
    for label in labels {
        let mut w = World::new();
        let mut b = w.bodies[2].clone();
        widen_grant(label, &mut b, &w);
        w.bodies[2] = b;
        let grants = w.grants();
        assert_eq!(run(&w, &grants), Err(Denial::Broadened), "{label}");
    }
}

// ---------------------------------------------------------------------------
// step 7: time
// ---------------------------------------------------------------------------

#[test]
fn step_7_not_yet_valid_and_skew() {
    let w = World::new();
    let grants = w.grants();
    let roots = w.roots();

    // The last grant starts at 2000. At 1900 it is not valid yet.
    let mut early = w.clone_inv_at(1_900);
    early.leaf = grants[2].id().unwrap();
    let inv = Invocation::sign(&w.keys[3], early).unwrap();
    let mut c = ctx(&w, &roots);
    c.now = 1_900;
    let rev = RevocationSet::current(1_900);
    assert_eq!(
        run_full(&grants, &inv, &c, &rev, &w.ledger()),
        Err(Denial::NotYetValid)
    );

    // A stated skew of 100 seconds admits it.
    c.skew_secs = 100;
    assert!(run_full(&grants, &inv, &c, &rev, &w.ledger()).is_ok());

    // A skew above the maximum is cut to it: 300 is not enough at 1500.
    let mut earlier = w.clone_inv_at(1_500);
    earlier.leaf = grants[2].id().unwrap();
    let inv = Invocation::sign(&w.keys[3], earlier).unwrap();
    c.now = 1_500;
    c.skew_secs = 10_000;
    let rev = RevocationSet::current(1_500);
    assert_eq!(
        run_full(&grants, &inv, &c, &rev, &w.ledger()),
        Err(Denial::NotYetValid)
    );
}

#[test]
fn step_7_expired_and_skew_never_extends_an_expiry() {
    let w = World::new();
    let grants = w.grants();
    let roots = w.roots();
    // The last grant ends at 8000; at exactly 8000 it is expired.
    let mut late = w.clone_inv_at(8_000);
    late.leaf = grants[2].id().unwrap();
    let inv = Invocation::sign(&w.keys[3], late).unwrap();
    let mut c = ctx(&w, &roots);
    c.now = 8_000;
    let rev = RevocationSet::current(8_000);
    assert_eq!(
        run_full(&grants, &inv, &c, &rev, &w.ledger()),
        Err(Denial::Expired)
    );
    c.skew_secs = 300;
    assert_eq!(
        run_full(&grants, &inv, &c, &rev, &w.ledger()),
        Err(Denial::Expired)
    );
}

// ---------------------------------------------------------------------------
// step 8: revocation
// ---------------------------------------------------------------------------

#[test]
fn step_8_a_revoked_grant_revokes_everything_below_it() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let c = ctx(&w, &roots);
    for (revoked, grant) in grants.iter().enumerate() {
        let mut rev = w.revocations();
        rev.revoke(grant.id().unwrap());
        assert_eq!(
            run_full(&grants, &inv, &c, &rev, &w.ledger()),
            Err(Denial::Revoked),
            "grant {revoked}"
        );
    }
}

#[test]
fn step_8_revocation_data_that_is_missing_or_too_old() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let mut c = ctx(&w, &roots);

    let none = RevocationSet::default();
    assert_eq!(
        run_full(&grants, &inv, &c, &none, &w.ledger()),
        Err(Denial::RevocationDataStale)
    );

    let old = RevocationSet::current(NOW - 1);
    assert_eq!(
        run_full(&grants, &inv, &c, &old, &w.ledger()),
        Err(Denial::RevocationDataStale)
    );

    // A bound for the declared effect class admits it; a bound for another
    // class does not.
    c.revocation_bounds = RevocationBounds {
        read: 3_600,
        reversible: 0,
        irreversible: 3_600,
    };
    assert_eq!(
        run_full(&grants, &inv, &c, &old, &w.ledger()),
        Err(Denial::RevocationDataStale)
    );
    c.revocation_bounds.reversible = 10;
    assert!(run_full(&grants, &inv, &c, &old, &w.ledger()).is_ok());
}

// ---------------------------------------------------------------------------
// step 9: the invocation
// ---------------------------------------------------------------------------

#[test]
fn step_9_a_malformed_invocation() {
    let mut w = World::new();
    w.inv.version = 2;
    expect(&w, &w.grants(), Denial::InvocationMalformed);

    let mut w = World::new();
    w.inv.res.clear();
    expect(&w, &w.grants(), Denial::InvocationMalformed);
}

#[test]
fn step_9_an_invocation_that_cites_another_chain() {
    let mut w = World::new();
    w.inv.leaf = [3u8; 32];
    expect(&w, &w.grants(), Denial::ChainMismatch);
}

#[test]
fn step_9_an_invoker_that_is_not_the_last_audience() {
    let mut w = World::new();
    w.inv.invoker = public(&w.keys[2]);
    let grants = w.grants();
    let inv = w.invocation_by(&grants, &w.keys[2]);
    let roots = w.roots();
    let c = ctx(&w, &roots);
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &w.ledger()),
        Err(Denial::InvokerNotAudience)
    );
}

#[test]
fn step_9_an_invocation_for_another_recipient_or_session() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();

    let mut c = ctx(&w, &roots);
    c.recipient = public(&key(40));
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &w.ledger()),
        Err(Denial::WrongRecipient)
    );

    let mut c = ctx(&w, &roots);
    c.session = [0x66u8; 32];
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &w.ledger()),
        Err(Denial::WrongSession)
    );
}

#[test]
fn step_9_an_invocation_that_is_too_old_or_from_the_future() {
    for time in [NOW - 61, NOW + 1] {
        let mut w = World::new();
        w.inv.time = time;
        expect(&w, &w.grants(), Denial::InvocationStale);
    }
    // The edges are accepted: exactly the maximum age, and a future time
    // within the stated skew.
    let mut w = World::new();
    w.inv.time = NOW - 60;
    assert!(run(&w, &w.grants()).is_ok());
    w.inv.time = NOW + 20;
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let mut c = ctx(&w, &roots);
    c.skew_secs = 20;
    assert!(run_full(&grants, &inv, &c, &w.revocations(), &w.ledger()).is_ok());
}

#[test]
fn step_9_an_invocation_signed_by_the_wrong_key() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation_by(&grants, &key(41));
    let roots = w.roots();
    let c = ctx(&w, &roots);
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &w.ledger()),
        Err(Denial::InvocationSignature)
    );

    // A body changed after signing.
    let mut inv = w.invocation(&grants);
    inv.body.cost = 0;
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &w.ledger()),
        Err(Denial::InvocationSignature)
    );
}

#[test]
fn step_9_a_nonce_that_was_used() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let c = ctx(&w, &roots);
    let mut ledger = MemoryLedger::new();
    ledger
        .charge(&Decision {
            chain_ids: Vec::new(),
            chain_limits: Vec::new(),
            chain_expiries: Vec::new(),
            nonce_horizon: NOW + 60,
            invoker: public(&w.keys[3]),
            nonce: [7u8; 16],
            now: NOW,
            effect: Effect::Read,
            cost: 0,
            bytes: 0,
        })
        .unwrap();
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &ledger),
        Err(Denial::NonceReplayed)
    );
}

// ---------------------------------------------------------------------------
// step 10: the operation is inside the last grant
// ---------------------------------------------------------------------------

#[test]
fn step_10_a_resource_the_last_grant_does_not_cover() {
    for path in [
        "/data/reports/q4",
        "/data/reports/q3/detail",
        "/data/reports",
        "/data",
    ] {
        let mut w = World::new();
        w.inv.res = vec![name(&w.keys[0], path)];
        expect(&w, &w.grants(), Denial::ResourceNotCovered);
    }
    // One covered and one not: every resource must be covered.
    let mut w = World::new();
    w.inv.res = vec![
        name(&w.keys[0], "/data/reports/q3"),
        name(&w.keys[0], "/data/reports/q4"),
    ];
    expect(&w, &w.grants(), Denial::ResourceNotCovered);
}

#[test]
fn step_10_an_action_the_last_grant_does_not_allow() {
    // The root and the middle grant allow `read` and `publish`; the last one
    // only `invoke`.
    for action in [Action::Read, Action::Publish, Action::Admin] {
        let mut w = World::new();
        w.inv.act = action;
        expect(&w, &w.grants(), Denial::ActionNotGranted);
    }
}

#[test]
fn step_10_an_effect_above_the_last_grants_ceiling() {
    // The root allows irreversible; the last grant only reversible.
    let mut w = World::new();
    w.inv.effect = Effect::Irreversible;
    expect(&w, &w.grants(), Denial::EffectExceeded);
}

// ---------------------------------------------------------------------------
// step 11: stateful limits
// ---------------------------------------------------------------------------

#[test]
fn step_11_a_limit_that_is_exhausted_anywhere_in_the_chain() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let c = ctx(&w, &roots);
    let ids: Vec<GrantId> = grants.iter().map(|g| g.id().unwrap()).collect();

    // (grant, calls, cost, bytes): each puts exactly one limit at its ceiling.
    let cases = [
        (0usize, 100u64, 0u64, 0u64),
        (0, 0, 1_000, 0),
        (0, 0, 0, 1_000_000),
        (1, 50, 0, 0),
        (2, 10, 0, 0),
        (2, 0, 100, 0),
        (2, 0, 0, 10_000),
    ];
    for (index, calls, cost, bytes) in cases {
        let mut ledger = MemoryLedger::new();
        ledger.preload(ids[index], calls, cost, bytes);
        assert_eq!(
            run_full(&grants, &inv, &c, &w.revocations(), &ledger),
            Err(Denial::LimitExhausted),
            "grant {index} calls {calls} cost {cost} bytes {bytes}"
        );
    }

    // One short of the ceiling still passes.
    let mut ledger = MemoryLedger::new();
    ledger.preload(ids[0], 99, 999, 999_990);
    assert!(run_full(&grants, &inv, &c, &w.revocations(), &ledger).is_ok());
}

#[test]
fn step_11_the_rate_limit_counts_a_rolling_window() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let c = ctx(&w, &roots);
    let last = grants[2].id().unwrap();

    // Two calls in the last minute use up a rate of 2 (this one would be 3rd).
    let mut ledger = MemoryLedger::new();
    ledger.record_call_at(last, NOW - 10);
    ledger.record_call_at(last, NOW - 20);
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &ledger),
        Err(Denial::LimitExhausted)
    );

    // Calls that fell out of the window do not count.
    let mut ledger = MemoryLedger::new();
    for _ in 0..5 {
        ledger.record_call_at(last, NOW - 100);
    }
    assert!(run_full(&grants, &inv, &c, &w.revocations(), &ledger).is_ok());
}

#[test]
fn step_11_a_ledger_that_cannot_say_refuses_only_where_a_limit_exists() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let c = ctx(&w, &roots);
    assert_eq!(
        run_full(&grants, &inv, &c, &w.revocations(), &UnknownLedger),
        Err(Denial::UsageUnknown)
    );

    // The same chain with no limits anywhere does not need the ledger.
    let mut w = World::new();
    for b in w.bodies.iter_mut() {
        b.limits = Limits::default();
    }
    let grants = w.grants();
    let inv = w.invocation(&grants);
    assert!(run_full(&grants, &inv, &c, &w.revocations(), &UnknownLedger).is_ok());
}

// ---------------------------------------------------------------------------
// the encoded entry point
// ---------------------------------------------------------------------------

#[test]
fn verify_encoded_agrees_with_verify_and_reports_decode_failures() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let c = ctx(&w, &roots);
    let chain_bytes: Vec<Vec<u8>> = grants.iter().map(|g| g.encode().unwrap()).collect();
    let inv_bytes = inv.encode().unwrap();

    let by_bytes = verify_encoded(&chain_bytes, &inv_bytes, &c, &w.revocations(), &w.ledger());
    assert_eq!(by_bytes, run(&w, &grants));
    assert!(by_bytes.is_ok());

    let mut bad_chain = chain_bytes.clone();
    bad_chain[1].push(0);
    assert_eq!(
        verify_encoded(&bad_chain, &inv_bytes, &c, &w.revocations(), &w.ledger()),
        Err(Denial::Malformed)
    );
    assert_eq!(
        verify_encoded(&[], &inv_bytes, &c, &w.revocations(), &w.ledger()),
        Err(Denial::ChainLimits)
    );
    let mut bad_inv = inv_bytes.clone();
    bad_inv.push(0);
    assert_eq!(
        verify_encoded(&chain_bytes, &bad_inv, &c, &w.revocations(), &w.ledger()),
        Err(Denial::InvocationMalformed)
    );
    let huge = vec![0u8; 100_000];
    assert_eq!(
        verify_encoded(&chain_bytes, &huge, &c, &w.revocations(), &w.ledger()),
        Err(Denial::InvocationLimits)
    );
}
