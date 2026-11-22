//! Revocation records, the feed head and the feed: one negative test per
//! failure mode, and step 8 of verification against a loaded feed.

mod common;

use common::*;
use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};
use spine_authority::{
    feed_digest, verify, Chain, Decision, DecodeError, Denial, Effect, FeedCheck, FeedError,
    FeedHead, FeedHeadBody, Grant, GrantId, Revocation, RevocationBody, RevocationBounds,
    RevocationFeed, RevocationView, FEED_HEAD_DOMAIN, FEED_HEAD_VERSION, MAX_FEED_HEAD_BYTES,
    MAX_FEED_REVOCATIONS, MAX_REVOCATION_BYTES, REVOCATION_DOMAIN, REVOCATION_ID_DOMAIN,
    REVOCATION_VERSION,
};

fn publisher() -> SigningKey {
    key(30)
}

fn mallory() -> SigningKey {
    key(20)
}

fn ids(grants: &[Grant]) -> Vec<GrantId> {
    grants.iter().map(|g| g.id().unwrap()).collect()
}

fn revoke(signer: &SigningKey, id: GrantId, at: u64, n: u8) -> Revocation {
    Revocation::issue(signer, id, at, [n; 16]).unwrap()
}

fn check(last: Option<u64>) -> FeedCheck {
    FeedCheck {
        publisher: public(&publisher()),
        last_seen_sequence: last,
        now: NOW,
    }
}

fn head_for(revs: &[Revocation], seq: u64, as_of: u64) -> FeedHead {
    FeedHead::issue(&publisher(), seq, as_of, revs).unwrap()
}

fn load(revs: Vec<Revocation>, seq: u64, as_of: u64) -> Result<RevocationFeed, FeedError> {
    let head = head_for(revs.as_slice(), seq, as_of);
    RevocationFeed::load(revs, &head, &check(None))
}

/// Verifies the standard invocation for an invocation of declared `effect`
/// against `feed` with the given bounds.
fn run_with(
    effect: Effect,
    feed: &dyn RevocationView,
    bounds: RevocationBounds,
) -> Result<Decision, Denial> {
    let mut w = World::new();
    w.inv.effect = effect;
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let mut c = ctx(&w, &roots);
    c.revocation_bounds = bounds;
    verify(&Chain::new(grants), &inv, &c, feed, &w.ledger())
}

fn run(feed: &dyn RevocationView) -> Result<Decision, Denial> {
    run_with(
        Effect::Reversible,
        feed,
        RevocationBounds {
            read: 100,
            reversible: 100,
            irreversible: 100,
        },
    )
}

// ---------------------------------------------------------------------------
// the record
// ---------------------------------------------------------------------------

#[test]
fn a_revocation_round_trips_and_has_the_documented_domains() {
    let w = World::new();
    let id = ids(&w.grants())[1];
    let r = revoke(&w.keys[1], id, 4_000, 7);
    assert!(REVOCATION_DOMAIN.ends_with(&[0u8]));
    assert!(REVOCATION_ID_DOMAIN.ends_with(&[0u8]));
    assert_eq!(r.body.revoker, public(&w.keys[1]));
    assert_eq!(r.body.grant, id);
    assert_eq!(r.body.revoked_at, 4_000);
    assert!(r
        .body
        .signing_bytes()
        .unwrap()
        .starts_with(REVOCATION_DOMAIN));
    assert_eq!(r.verify(), Ok(()));

    let bytes = r.encode().unwrap();
    assert!(bytes.len() <= MAX_REVOCATION_BYTES);
    assert_eq!(Revocation::decode(&bytes).unwrap(), r);

    let mut hasher = Sha256::new();
    hasher.update(REVOCATION_ID_DOMAIN);
    hasher.update(bytes.as_slice());
    let expected: [u8; 32] = hasher.finalize().into();
    assert_eq!(r.id().unwrap(), expected);
}

#[test]
fn a_revocation_with_a_bad_signature_is_refused() {
    let w = World::new();
    let id = ids(&w.grants())[1];
    let mut r = revoke(&w.keys[1], id, 4_000, 7);
    r.signature[0] ^= 1;
    assert!(!r.verify_signature());
    assert_eq!(r.verify(), Err(FeedError::BadRevocationSignature));
}

#[test]
fn a_revocation_signed_by_someone_else_is_refused() {
    let w = World::new();
    let id = ids(&w.grants())[1];
    let body = RevocationBody {
        version: REVOCATION_VERSION,
        revoker: public(&w.keys[1]),
        grant: id,
        revoked_at: 4_000,
        nonce: [7u8; 16],
    };
    let r = Revocation::sign(&mallory(), body).unwrap();
    assert_eq!(r.verify(), Err(FeedError::BadRevocationSignature));
}

#[test]
fn tampering_with_any_field_of_a_revocation_breaks_the_signature() {
    let w = World::new();
    let id = ids(&w.grants())[1];
    let original = revoke(&w.keys[1], id, 4_000, 7);

    let mut r = original.clone();
    r.body.grant = ids(&w.grants())[2];
    assert_eq!(r.verify(), Err(FeedError::BadRevocationSignature));

    let mut r = original.clone();
    r.body.revoked_at += 1;
    assert_eq!(r.verify(), Err(FeedError::BadRevocationSignature));

    let mut r = original.clone();
    r.body.nonce[0] ^= 1;
    assert_eq!(r.verify(), Err(FeedError::BadRevocationSignature));

    let mut r = original.clone();
    r.body.revoker = public(&w.keys[0]);
    assert_eq!(r.verify(), Err(FeedError::BadRevocationSignature));
}

#[test]
fn an_unknown_revocation_version_is_refused() {
    let w = World::new();
    let body = RevocationBody {
        version: 2,
        revoker: public(&w.keys[1]),
        grant: [1u8; 32],
        revoked_at: 1,
        nonce: [0u8; 16],
    };
    let r = Revocation::sign(&w.keys[1], body).unwrap();
    assert_eq!(r.verify(), Err(FeedError::UnknownVersion));
}

#[test]
fn revocation_decoding_refuses_what_is_not_the_canonical_encoding() {
    let w = World::new();
    let r = revoke(&w.keys[1], [3u8; 32], 4_000, 7);
    let bytes = r.encode().unwrap();

    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(Revocation::decode(&trailing).is_err());
    assert!(Revocation::decode(&bytes[..bytes.len() - 1]).is_err());
    assert!(Revocation::decode(&with_extra_field(&bytes)).is_err());
    assert!(Revocation::decode(&with_long_form_version(&bytes)).is_err());
    assert_eq!(
        Revocation::decode(&[0u8; MAX_REVOCATION_BYTES + 1]),
        Err(DecodeError::TooLarge(MAX_REVOCATION_BYTES))
    );
}

#[test]
fn a_revocation_whose_ids_are_arrays_of_numbers_is_refused() {
    // A grant id written as an array of 32 integers instead of a byte string.
    let w = World::new();
    let r = revoke(&w.keys[1], [3u8; 32], 4_000, 7);
    let bytes = r.encode().unwrap();
    let marker = [0x58u8, 0x20, 3, 3, 3, 3];
    let at = (0..bytes.len() - marker.len())
        .find(|i| bytes[*i..*i + marker.len()] == marker)
        .expect("the grant id is in the encoding");
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(&[0x98, 0x20]);
    out.extend_from_slice(&[3u8; 32]);
    out.extend_from_slice(&bytes[at + 34..]);
    assert!(Revocation::decode(&out).is_err());
}

// ---------------------------------------------------------------------------
// who may revoke
// ---------------------------------------------------------------------------

#[test]
fn the_issuer_of_a_grant_or_of_any_grant_above_it_may_revoke_it() {
    let w = World::new();
    let chain = w.chain();
    let id = ids(&chain.grants);
    // (revoker key index, revoked grant index, allowed). The grants are
    // issued by keys 0, 1 and 2; key 3 only holds the last one.
    let cases = [
        (0usize, 0usize, true),
        (0, 1, true),
        (0, 2, true),
        (1, 0, false),
        (1, 1, true),
        (1, 2, true),
        (2, 0, false),
        (2, 1, false),
        (2, 2, true),
        (3, 0, false),
        (3, 1, false),
        (3, 2, false),
    ];
    for (revoker, target, allowed) in cases {
        let r = revoke(&w.keys[revoker], id[target], 4_000, 1);
        assert_eq!(
            r.is_authorized_by(&chain),
            allowed,
            "revoker {revoker} grant {target}"
        );
    }
}

#[test]
fn a_stranger_or_an_unknown_grant_is_not_authorized() {
    let w = World::new();
    let chain = w.chain();
    let id = ids(&chain.grants);
    assert!(!revoke(&mallory(), id[2], 4_000, 1).is_authorized_by(&chain));
    assert!(!revoke(&w.keys[0], [0x77u8; 32], 4_000, 1).is_authorized_by(&chain));
    assert!(!revoke(&w.keys[0], id[2], 4_000, 1).is_authorized_by(&Chain::new(Vec::new())));
}

// ---------------------------------------------------------------------------
// the head
// ---------------------------------------------------------------------------

#[test]
fn a_head_round_trips_and_describes_the_set() {
    let revs = vec![
        revoke(&key(1), [1u8; 32], 100, 1),
        revoke(&key(2), [2u8; 32], 200, 2),
    ];
    let head = head_for(&revs, 7, 300);
    assert!(FEED_HEAD_DOMAIN.ends_with(&[0u8]));
    assert_eq!(head.body.version, FEED_HEAD_VERSION);
    assert_eq!(head.body.publisher, public(&publisher()));
    assert_eq!(head.body.sequence, 7);
    assert_eq!(head.body.as_of, 300);
    assert_eq!(head.body.count, 2);
    assert!(head
        .body
        .signing_bytes()
        .unwrap()
        .starts_with(FEED_HEAD_DOMAIN));
    assert_eq!(head.verify(&public(&publisher())), Ok(()));
    assert_eq!(head.check_set(&revs), Ok(()));

    let bytes = head.encode().unwrap();
    assert!(bytes.len() <= MAX_FEED_HEAD_BYTES);
    assert_eq!(FeedHead::decode(&bytes).unwrap(), head);

    // The digest does not depend on the order of the set.
    let reversed = vec![revs[1].clone(), revs[0].clone()];
    assert_eq!(head.check_set(&reversed), Ok(()));
    let ids: Vec<[u8; 32]> = revs.iter().map(|r| r.id().unwrap()).collect();
    assert_eq!(head.body.digest, feed_digest(&ids));
}

#[test]
fn a_head_from_another_publisher_is_refused() {
    let head = head_for(&[], 1, NOW);
    assert_eq!(
        head.verify(&public(&mallory())),
        Err(FeedError::WrongPublisher)
    );
    let other = FeedHead::issue(&mallory(), 1, NOW, &[]).unwrap();
    assert_eq!(
        RevocationFeed::load(Vec::new(), &other, &check(None)).unwrap_err(),
        FeedError::WrongPublisher
    );
}

#[test]
fn a_head_with_a_bad_signature_or_a_tampered_field_is_refused() {
    let mut head = head_for(&[], 1, NOW - 10);
    head.signature[3] ^= 1;
    assert_eq!(
        RevocationFeed::load(Vec::new(), &head, &check(None)).unwrap_err(),
        FeedError::BadHeadSignature
    );

    for tamper in 0..4 {
        let mut head = head_for(&[], 1, NOW - 10);
        match tamper {
            0 => head.body.sequence += 1,
            1 => head.body.as_of += 1,
            2 => head.body.count += 1,
            _ => head.body.digest[0] ^= 1,
        }
        assert_eq!(
            RevocationFeed::load(Vec::new(), &head, &check(None)).unwrap_err(),
            FeedError::BadHeadSignature,
            "tamper {tamper}"
        );
    }
}

#[test]
fn an_unknown_head_version_is_refused() {
    let body = FeedHeadBody {
        version: 2,
        publisher: public(&publisher()),
        sequence: 1,
        as_of: NOW,
        count: 0,
        digest: feed_digest(&[]),
    };
    let head = FeedHead::sign(&publisher(), body).unwrap();
    assert_eq!(
        head.verify(&public(&publisher())),
        Err(FeedError::UnknownVersion)
    );
}

#[test]
fn head_decoding_refuses_what_is_not_the_canonical_encoding() {
    let bytes = head_for(&[], 1, NOW).encode().unwrap();
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(FeedHead::decode(&trailing).is_err());
    assert!(FeedHead::decode(&with_extra_field(&bytes)).is_err());
    assert!(FeedHead::decode(&with_long_form_version(&bytes)).is_err());
    assert_eq!(
        FeedHead::decode(&[0u8; MAX_FEED_HEAD_BYTES + 1]),
        Err(DecodeError::TooLarge(MAX_FEED_HEAD_BYTES))
    );
}

#[test]
fn a_head_dated_in_the_future_is_refused() {
    assert_eq!(
        load(Vec::new(), 1, NOW + 1).unwrap_err(),
        FeedError::FutureHead
    );
    assert!(load(Vec::new(), 1, NOW).is_ok());
}

#[test]
fn a_sequence_that_does_not_advance_is_a_rollback() {
    let head = head_for(&[], 5, NOW);
    for last in [5u64, 6, 100] {
        assert_eq!(
            RevocationFeed::load(Vec::new(), &head, &check(Some(last))).unwrap_err(),
            FeedError::SequenceRollback,
            "last seen {last}"
        );
    }
    let feed = RevocationFeed::load(Vec::new(), &head, &check(Some(4))).unwrap();
    assert_eq!(feed.sequence(), Some(5));
    assert!(RevocationFeed::load(Vec::new(), &head, &check(None)).is_ok());
}

#[test]
fn a_set_that_does_not_match_the_head_is_refused() {
    let a = revoke(&key(1), [1u8; 32], 100, 1);
    let b = revoke(&key(2), [2u8; 32], 100, 2);
    let c = revoke(&key(3), [3u8; 32], 100, 3);
    let head = head_for(&[a.clone(), b.clone()], 1, NOW);
    let try_set = |set: Vec<Revocation>| RevocationFeed::load(set, &head, &check(None));

    // Truncated.
    assert_eq!(
        try_set(vec![a.clone()]).unwrap_err(),
        FeedError::CountMismatch
    );
    assert_eq!(try_set(Vec::new()).unwrap_err(), FeedError::CountMismatch);
    // Padded.
    assert_eq!(
        try_set(vec![a.clone(), b.clone(), c.clone()]).unwrap_err(),
        FeedError::CountMismatch
    );
    // Same size, one record swapped.
    assert_eq!(
        try_set(vec![a.clone(), c.clone()]).unwrap_err(),
        FeedError::DigestMismatch
    );
    // The same set in another order is the same set.
    assert!(try_set(vec![b.clone(), a.clone()]).is_ok());
}

#[test]
fn a_replayed_head_does_not_vouch_for_a_newer_set() {
    let a = revoke(&key(1), [1u8; 32], 100, 1);
    let b = revoke(&key(2), [2u8; 32], 100, 2);
    let old_head = head_for(&[a.clone()], 1, NOW - 1_000);
    // The newer set holds b as well: the old head does not describe it.
    assert_eq!(
        RevocationFeed::load(vec![a.clone(), b], &old_head, &check(None)).unwrap_err(),
        FeedError::CountMismatch
    );
    // The old head with its own set is genuine but old: once a newer head has
    // been seen, its sequence is refused.
    assert_eq!(
        RevocationFeed::load(vec![a], &old_head, &check(Some(2))).unwrap_err(),
        FeedError::SequenceRollback
    );
}

#[test]
fn duplicates_and_oversize_sets_are_refused() {
    let a = revoke(&key(1), [1u8; 32], 100, 1);
    assert_eq!(
        load(vec![a.clone(), a.clone()], 1, NOW).unwrap_err(),
        FeedError::DuplicateRevocation
    );

    let head = head_for(&[a.clone()], 1, NOW);
    let many = vec![a; MAX_FEED_REVOCATIONS + 1];
    assert_eq!(
        RevocationFeed::load(many, &head, &check(None)).unwrap_err(),
        FeedError::TooMany
    );
}

#[test]
fn a_bad_record_inside_a_matching_set_is_refused() {
    let mut bad = revoke(&key(1), [1u8; 32], 100, 1);
    bad.signature[0] ^= 1;
    assert_eq!(
        load(vec![bad], 1, NOW).unwrap_err(),
        FeedError::BadRevocationSignature
    );

    let body = RevocationBody {
        version: 2,
        revoker: public(&key(1)),
        grant: [1u8; 32],
        revoked_at: 100,
        nonce: [1u8; 16],
    };
    let old = Revocation::sign(&key(1), body).unwrap();
    assert_eq!(
        load(vec![old], 1, NOW).unwrap_err(),
        FeedError::UnknownVersion
    );

    let later = revoke(&key(1), [1u8; 32], NOW + 1, 1);
    assert_eq!(
        load(vec![later], 1, NOW).unwrap_err(),
        FeedError::RevokedAfterHead
    );
}

#[test]
fn a_feed_without_data_has_no_age() {
    let feed = RevocationFeed::default();
    assert_eq!(feed.data_age_secs(NOW), None);
    assert!(feed.is_empty());
    assert_eq!(feed.sequence(), None);
    assert_eq!(run(&feed), Err(Denial::RevocationDataStale));
}

// ---------------------------------------------------------------------------
// step 8 against a feed
// ---------------------------------------------------------------------------

#[test]
fn an_empty_current_feed_lets_the_chain_through() {
    let feed = load(Vec::new(), 1, NOW).unwrap();
    assert!(run(&feed).is_ok());
}

#[test]
fn revoking_a_middle_grant_refuses_the_leaf_at_step_8() {
    let w = World::new();
    let id = ids(&w.grants());
    // The issuer of grant 1 revokes it; the leaf is below it.
    let feed = load(vec![revoke(&w.keys[1], id[1], NOW - 10, 1)], 1, NOW).unwrap();
    let denial = run(&feed).unwrap_err();
    assert_eq!(denial, Denial::Revoked);
    assert_eq!(denial.step(), 8);
    assert_eq!(feed.len(), 1);
}

#[test]
fn the_root_issuer_and_the_leaf_issuer_can_revoke_too() {
    let w = World::new();
    let id = ids(&w.grants());
    for (revoker, target) in [(0usize, 0usize), (0, 2), (2, 2), (1, 2)] {
        let feed = load(
            vec![revoke(&w.keys[revoker], id[target], NOW - 10, 1)],
            1,
            NOW,
        )
        .unwrap();
        assert_eq!(
            run(&feed),
            Err(Denial::Revoked),
            "revoker {revoker} grant {target}"
        );
    }
}

#[test]
fn a_revocation_by_someone_without_the_right_is_ignored_at_step_8() {
    let w = World::new();
    let id = ids(&w.grants());
    let ignored = [
        (mallory(), id[1]),
        // The holder of the last grant did not issue any grant of the chain.
        (w.keys[3].clone(), id[2]),
        // Key 2 issued only the grant below grant 1.
        (w.keys[2].clone(), id[1]),
        (w.keys[1].clone(), id[0]),
    ];
    for (signer, target) in ignored {
        let feed = load(vec![revoke(&signer, target, NOW - 10, 1)], 1, NOW).unwrap();
        assert!(run(&feed).is_ok());
        // The bare id test errs toward refusal; only the chain-aware step 8
        // looks at who signed.
        assert!(feed.is_revoked(&target));
    }
}

#[test]
fn a_revocation_of_a_grant_outside_the_chain_does_nothing() {
    let w = World::new();
    let feed = load(vec![revoke(&w.keys[0], [0x42u8; 32], NOW - 10, 1)], 1, NOW).unwrap();
    assert!(run(&feed).is_ok());
}

#[test]
fn stale_revocation_data_is_refused_per_effect_class() {
    let feed = load(Vec::new(), 1, NOW - 60).unwrap();
    let bounds = RevocationBounds {
        read: 100,
        reversible: 50,
        irreversible: 0,
    };
    // Age 60: read allows 100, reversible only 50.
    assert!(run_with(Effect::Read, &feed, bounds).is_ok());
    assert_eq!(
        run_with(Effect::Reversible, &feed, bounds),
        Err(Denial::RevocationDataStale)
    );
    let looser = RevocationBounds {
        reversible: 60,
        ..bounds
    };
    assert!(run_with(Effect::Reversible, &feed, looser).is_ok());

    // Irreversible with one second of age and a bound of 0 is stale at step 8,
    // before step 10 would refuse the class itself.
    let almost = load(Vec::new(), 1, NOW - 1).unwrap();
    assert_eq!(
        run_with(Effect::Irreversible, &almost, bounds),
        Err(Denial::RevocationDataStale)
    );
    let fresh = load(Vec::new(), 1, NOW).unwrap();
    assert_eq!(
        run_with(Effect::Irreversible, &fresh, bounds),
        Err(Denial::EffectExceeded)
    );
}

#[test]
fn the_default_bounds_refuse_any_age() {
    let feed = load(Vec::new(), 1, NOW - 1).unwrap();
    for effect in [Effect::Read, Effect::Reversible, Effect::Irreversible] {
        assert_eq!(
            run_with(effect, &feed, RevocationBounds::default()),
            Err(Denial::RevocationDataStale),
            "{effect:?}"
        );
    }
    let current = load(Vec::new(), 1, NOW).unwrap();
    assert!(run_with(Effect::Read, &current, RevocationBounds::default()).is_ok());
}

#[test]
fn a_revoked_grant_is_refused_even_when_the_data_is_stale_enough_to_pass() {
    let w = World::new();
    let id = ids(&w.grants());
    let feed = load(vec![revoke(&w.keys[0], id[0], NOW - 500, 1)], 1, NOW - 50).unwrap();
    assert_eq!(run(&feed), Err(Denial::Revoked));
}
