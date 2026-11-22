//! The whole path: issue a chain, invoke, verify, charge, receipt; then
//! revoke a middle grant and refuse the leaf at step 8.

mod common;

use common::*;
use ed25519_dalek::SigningKey;
use spine_authority::{
    verify, Action, Chain, ChargeError, Denial, Effect, FeedCheck, FeedError, FeedHead, Grant,
    GrantSpec, Limits, MemoryLedger, Receipt, ReceiptError, Revocation, RevocationBounds,
    RevocationFeed, UsageLedger,
};

fn calls(n: u64) -> Limits {
    Limits {
        calls: Some(n),
        ..Limits::default()
    }
}

/// Issues the standard three links with the real builders: key 0 owns
/// `/data` and delegates to key 1, then 2, then 3. The last grant allows two
/// calls.
fn issue_chain(keys: &[SigningKey]) -> Vec<Grant> {
    let g0 = Grant::issue_root(
        &keys[0],
        GrantSpec {
            aud: public(&keys[1]),
            role: "principal".to_string(),
            res: vec![prefix(&keys[0], "/data")],
            act: vec![Action::Read, Action::Invoke],
            effect: Effect::Reversible,
            delegable: 2,
            nbf: 1_000,
            exp: 10_000,
            limits: calls(5),
            caveats: Vec::new(),
            nonce: [1u8; 16],
        },
    )
    .unwrap();
    let g1 = Grant::delegate(
        &keys[1],
        &g0,
        GrantSpec {
            aud: public(&keys[2]),
            role: "operator".to_string(),
            res: vec![prefix(&keys[0], "/data/reports")],
            act: vec![Action::Invoke],
            effect: Effect::Reversible,
            delegable: 1,
            nbf: 1_000,
            exp: 9_000,
            limits: calls(3),
            caveats: Vec::new(),
            nonce: [2u8; 16],
        },
    )
    .unwrap();
    let g2 = Grant::delegate(
        &keys[2],
        &g1,
        GrantSpec {
            aud: public(&keys[3]),
            role: "agent".to_string(),
            res: vec![exact(&keys[0], "/data/reports/q3")],
            act: vec![Action::Invoke],
            effect: Effect::Reversible,
            delegable: 0,
            nbf: 2_000,
            exp: 8_000,
            limits: calls(2),
            caveats: Vec::new(),
            nonce: [3u8; 16],
        },
    )
    .unwrap();
    vec![g0, g1, g2]
}

fn publisher() -> SigningKey {
    key(30)
}

fn feed_check(last_seen_sequence: Option<u64>) -> FeedCheck {
    FeedCheck {
        publisher: public(&publisher()),
        last_seen_sequence,
        now: NOW,
    }
}

fn feed_of(revs: Vec<Revocation>, seq: u64, as_of: u64) -> RevocationFeed {
    let head = FeedHead::issue(&publisher(), seq, as_of, revs.as_slice()).unwrap();
    RevocationFeed::load(revs, &head, &feed_check(None)).unwrap()
}

#[test]
fn issue_invoke_verify_charge_receipt() {
    let mut w = World::new();
    let grants = issue_chain(&w.keys);
    let chain = Chain::new(grants.clone());
    let feed = feed_of(Vec::new(), 1, NOW);
    let mut ledger = MemoryLedger::new();
    let roots = w.roots();
    let recipient = public(&w.recipient);

    for n in 1..=2u8 {
        w.inv.nonce = [n; 16];
        let inv = w.invocation(&grants);
        let c = ctx(&w, &roots);
        let verdict = verify(&chain, &inv, &c, &feed, &ledger);
        let decision = verdict.clone().unwrap();
        assert_eq!(decision.chain_ids.len(), 3);

        // Charge and record the nonce in one step, then receipt.
        ledger.charge(&decision).unwrap();
        let receipt = Receipt::issue(&w.recipient, &inv, &verdict, [0xee; 32], NOW).unwrap();
        assert_eq!(receipt.verify(&recipient), Ok(()));
        assert_eq!(receipt.matches(&inv, &verdict), Ok(()));
        assert_eq!(receipt.body.leaf, grants[2].id().unwrap());
        assert_eq!(receipt.body.invocation, inv.id().unwrap());
    }

    // Every grant of the chain was charged twice.
    for grant in &grants {
        let spent = ledger.spent(&grant.id().unwrap(), NOW).unwrap();
        assert_eq!(spent.calls, 2);
    }

    // The same invocation again is a replay; a fresh one meets the last
    // grant's ceiling of two calls at step 11.
    let inv = w.invocation(&grants);
    let c = ctx(&w, &roots);
    assert_eq!(
        verify(&chain, &inv, &c, &feed, &ledger),
        Err(Denial::NonceReplayed)
    );
    w.inv.nonce = [3u8; 16];
    let inv = w.invocation(&grants);
    let verdict = verify(&chain, &inv, &c, &feed, &ledger);
    assert_eq!(verdict, Err(Denial::LimitExhausted));

    // The refusal is receipted with its step and nothing more.
    let receipt = Receipt::issue(&w.recipient, &inv, &verdict, [0xee; 32], NOW).unwrap();
    assert_eq!(receipt.body.step, Some(11));
    assert!(!receipt.body.allowed);
    assert_eq!(receipt.verify(&recipient), Ok(()));
    assert_eq!(receipt.matches(&inv, &verdict), Ok(()));

    // And a late charge of an already verified decision cannot overspend.
    let mut racing = MemoryLedger::new();
    w.inv.nonce = [4u8; 16];
    let inv = w.invocation(&grants);
    let decision = verify(&chain, &inv, &c, &feed, &racing).unwrap();
    racing.charge(&decision).unwrap();
    let mut second = decision.clone();
    second.nonce = [5u8; 16];
    racing.charge(&second).unwrap();
    let mut third = decision;
    third.nonce = [6u8; 16];
    assert_eq!(racing.charge(&third), Err(ChargeError::LimitExhausted));
    assert!(!racing.nonce_seen(&third.invoker, &third.nonce));
}

#[test]
fn a_receipt_checks_out_only_for_its_own_invocation_and_recipient() {
    let mut w = World::new();
    let grants = issue_chain(&w.keys);
    let chain = Chain::new(grants.clone());
    let feed = feed_of(Vec::new(), 1, NOW);
    let ledger = MemoryLedger::new();
    let roots = w.roots();
    let c = ctx(&w, &roots);

    w.inv.nonce = [1u8; 16];
    let inv_a = w.invocation(&grants);
    let verdict_a = verify(&chain, &inv_a, &c, &feed, &ledger);
    w.inv.nonce = [2u8; 16];
    let inv_b = w.invocation(&grants);
    let verdict_b = verify(&chain, &inv_b, &c, &feed, &ledger);

    let receipt = Receipt::issue(&w.recipient, &inv_a, &verdict_a, [1u8; 32], NOW).unwrap();
    assert_eq!(receipt.matches(&inv_a, &verdict_a), Ok(()));
    assert_eq!(
        receipt.matches(&inv_b, &verdict_b),
        Err(ReceiptError::InvocationMismatch)
    );
    assert_eq!(
        receipt.verify(&public(&w.keys[0])),
        Err(ReceiptError::WrongRecipient)
    );
}

#[test]
fn revoking_a_middle_grant_refuses_the_leaf_and_the_head_moves_forward() {
    let mut w = World::new();
    let grants = issue_chain(&w.keys);
    let chain = Chain::new(grants.clone());
    let roots = w.roots();
    let ledger = MemoryLedger::new();
    let c = ctx(&w, &roots);
    w.inv.nonce = [1u8; 16];
    let inv = w.invocation(&grants);

    let before = feed_of(Vec::new(), 1, NOW - 5);
    let mut loose = c.clone();
    loose.revocation_bounds = RevocationBounds {
        read: 100,
        reversible: 100,
        irreversible: 100,
    };
    assert!(verify(&chain, &inv, &loose, &before, &ledger).is_ok());

    // The issuer of the middle grant revokes it. The caller accepts the next
    // head, whose sequence is above the one it saw.
    let revocation =
        Revocation::issue(&w.keys[1], grants[1].id().unwrap(), NOW - 1, [9u8; 16]).unwrap();
    let revs = vec![revocation];
    let head = FeedHead::issue(&publisher(), 2, NOW, revs.as_slice()).unwrap();
    let after = RevocationFeed::load(revs.clone(), &head, &feed_check(before.sequence())).unwrap();

    // The leaf, below the revoked grant, is refused at step 8, and the
    // denial carries only the step.
    let verdict = verify(&chain, &inv, &loose, &after, &ledger);
    assert_eq!(verdict, Err(Denial::Revoked));
    let receipt = Receipt::issue(&w.recipient, &inv, &verdict, [0u8; 32], NOW).unwrap();
    assert_eq!(receipt.body.step, Some(8));

    // Replaying the old head against the new set, or the old head after the
    // new one was seen, is refused.
    let old_head = FeedHead::issue(&publisher(), 1, NOW - 5, &[]).unwrap();
    assert_eq!(
        RevocationFeed::load(revs, &old_head, &feed_check(None)).unwrap_err(),
        FeedError::CountMismatch
    );
    assert_eq!(
        RevocationFeed::load(Vec::new(), &old_head, &feed_check(after.sequence())).unwrap_err(),
        FeedError::SequenceRollback
    );
}

#[test]
fn revocation_data_that_is_too_old_for_the_effect_class_refuses_at_step_8() {
    let mut w = World::new();
    let grants = issue_chain(&w.keys);
    let chain = Chain::new(grants.clone());
    let roots = w.roots();
    let ledger = MemoryLedger::new();
    let mut c = ctx(&w, &roots);
    w.inv.nonce = [1u8; 16];

    // Data as of 40 seconds ago; reads may use data up to a minute old,
    // reversible effects only 30 seconds.
    let feed = feed_of(Vec::new(), 1, NOW - 40);
    c.revocation_bounds = RevocationBounds {
        read: 60,
        reversible: 30,
        irreversible: 0,
    };

    w.inv.effect = Effect::Read;
    let inv = w.invocation(&grants);
    assert!(verify(&chain, &inv, &c, &feed, &ledger).is_ok());

    w.inv.effect = Effect::Reversible;
    let inv = w.invocation(&grants);
    assert_eq!(
        verify(&chain, &inv, &c, &feed, &ledger),
        Err(Denial::RevocationDataStale)
    );

    // No data at all is stale for every class.
    let none = RevocationFeed::default();
    w.inv.effect = Effect::Read;
    let inv = w.invocation(&grants);
    assert_eq!(
        verify(&chain, &inv, &c, &none, &ledger),
        Err(Denial::RevocationDataStale)
    );
}
