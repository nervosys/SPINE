//! Key succession wired into the store, the resolver and the persistence layer
//! (spec 02, section 10 and 10.11): admission against a held log, the bounded
//! log book, equivocation failing closed across a restart, and the signing path
//! for a rotated name.

use std::collections::HashSet;

use ed25519_dalek::SigningKey;
use spine_name::{
    log_within_bounds, publication_log, publication_value, Endpoint, LocalResolver, LogBook,
    LogChange, LogOffer, NameError, NameRecord, PutOutcome, RecordStore, Resolver, SpineUri,
    SuccessionLog, MAX_EQUIVOCATION_PROOFS, MAX_SUCCESSION_ENTRIES, MAX_SUCCESSION_LOGS,
    SUCCESSION_META_KEY,
};

const NOW: u64 = 1_000;

fn sk(n: u8) -> SigningKey {
    SigningKey::from_bytes(&[n; 32])
}

fn pk(k: &SigningKey) -> [u8; 32] {
    k.verifying_key().to_bytes()
}

fn record(name_key: &[u8; 32], seq: u64) -> NameRecord {
    NameRecord::new(SpineUri::did(*name_key), seq, NOW)
        .unwrap()
        .with_ttl(100)
        .with_endpoint(Endpoint::new("tcp", "h:1"))
}

/// A record signed by the inception key alone, as every name has always been.
fn plain(key: &SigningKey, seq: u64) -> NameRecord {
    let mut r = record(&pk(key), seq);
    r.sign(key).unwrap();
    r
}

/// A record signed by the current key of `log`, with its head claim.
fn current(
    inception: &SigningKey,
    signer: &SigningKey,
    log: &SuccessionLog,
    seq: u64,
) -> NameRecord {
    let state = log.verify(&pk(inception)).unwrap();
    let mut r = record(&pk(inception), seq);
    r.sign_current(signer, &state).unwrap();
    r
}

/// Sign whatever the record holds, with any key, bypassing every check `sign` and
/// `sign_current` make: what an attacker or an old client would send.
fn raw_sign(mut r: NameRecord, key: &SigningKey) -> NameRecord {
    let sig: ed25519_dalek::Signature = ed25519_dalek::Signer::sign(key, &r.signing_bytes());
    r.signature = sig.to_bytes().to_vec();
    r
}

/// Inception key sk(1); entry 1 reveals sk(2) and commits to sk(3).
fn log_one() -> SuccessionLog {
    SuccessionLog::default()
        .rotated(&pk(&sk(1)), &sk(1), &pk(&sk(2)), &pk(&sk(3)), 1_000)
        .unwrap()
}

/// Entry 2 reveals sk(3) (the committed key) and commits to sk(4).
fn log_two() -> SuccessionLog {
    log_one()
        .rotated(&pk(&sk(1)), &sk(2), &pk(&sk(3)), &pk(&sk(4)), 1_001)
        .unwrap()
}

fn log_retired() -> SuccessionLog {
    log_one()
        .retired(&pk(&sk(1)), &sk(2), &pk(&sk(3)), 1_002)
        .unwrap()
}

/// A different entry 1 for the same name: the inception key signs two entries.
fn log_fork() -> SuccessionLog {
    SuccessionLog::default()
        .rotated(&pk(&sk(1)), &sk(1), &pk(&sk(7)), &pk(&sk(8)), 1_000)
        .unwrap()
}

#[test]
fn a_name_without_a_log_is_admitted_exactly_as_verify_does() {
    let mut store = RecordStore::new();
    let good = plain(&sk(1), 1);
    assert!(store.logs().is_empty());
    assert_eq!(store.put(good.clone(), NOW).unwrap(), PutOutcome::Inserted);
    // The book stayed empty: admission never consulted or created a log.
    assert!(store.logs().is_empty());
    assert_eq!(store.logs().log_count(), 0);
    assert_eq!(store.logs().authorise(&good), good.verify());

    let mut bad = plain(&sk(1), 2);
    bad.seq += 1;
    assert_eq!(store.logs().authorise(&bad), bad.verify());
    assert_eq!(store.put(bad, NOW), Err(NameError::BadSignature));

    // A non-did name is refused with the error `verify` gives.
    let host = NameRecord::for_host(
        SpineUri::parse("spine://host:seed.example.org:9440/").unwrap(),
        NOW,
        60,
    )
    .unwrap();
    assert_eq!(store.logs().authorise(&host), host.verify());
}

#[test]
fn an_empty_or_repeated_log_changes_nothing() {
    let mut store = RecordStore::new();
    assert_eq!(
        store.offer_log(SuccessionLog::default()).unwrap(),
        LogChange::Unchanged
    );
    assert!(store.logs().is_empty());
    store.offer_log(log_two()).unwrap();
    // A prefix of the held log and the held log itself: no change, no work.
    assert_eq!(store.offer_log(log_one()).unwrap(), LogChange::Unchanged);
    assert_eq!(store.offer_log(log_two()).unwrap(), LogChange::Unchanged);
    assert_eq!(store.logs().state(&pk(&sk(1))).unwrap().index(), 2);
}

#[test]
fn an_invalid_log_is_refused_and_not_kept() {
    let mut store = RecordStore::new();
    let mut tampered = log_one();
    tampered.entries[0].effective_at += 1;
    assert!(matches!(
        store.offer_log(tampered),
        Err(NameError::InvalidSuccessionLog(_))
    ));
    assert!(store.logs().is_empty());
}

#[test]
fn a_rotated_name_refuses_the_old_key_once_the_log_is_held() {
    let mut store = RecordStore::new();
    let old = plain(&sk(1), 1);
    assert_eq!(store.put(old.clone(), NOW).unwrap(), PutOutcome::Inserted);

    assert_eq!(
        store.offer_log(log_one()).unwrap(),
        LogChange::Adopted {
            inception_key: pk(&sk(1))
        }
    );
    // The record the old key signed is no longer held.
    assert!(store.get(&old.name).is_none());

    // The old key can no longer publish for the name, whatever the seq.
    let newer_by_old_key = plain(&sk(1), 9);
    assert!(matches!(
        store.put(newer_by_old_key, NOW),
        Err(NameError::SupersededKey(_))
    ));

    // The current key, with the head claim, is admitted.
    let fresh = current(&sk(1), &sk(2), &log_one(), 2);
    assert!(fresh.meta.contains_key(SUCCESSION_META_KEY));
    assert_eq!(store.put(fresh.clone(), NOW).unwrap(), PutOutcome::Inserted);
    assert_eq!(store.get(&fresh.name), Some(&fresh));

    // The current key without a claim is also admitted: the claim is optional.
    let mut bare = record(&pk(&sk(1)), 3);
    bare.sign(&sk(2)).unwrap_err(); // `sign` still only takes the name key
    let state = log_one().verify(&pk(&sk(1))).unwrap();
    bare.sign_current(&sk(2), &state).unwrap();
    bare.meta.remove(SUCCESSION_META_KEY);
    let bare = raw_sign(bare, &sk(2));
    assert_eq!(store.put(bare, NOW).unwrap(), PutOutcome::Updated);
}

#[test]
fn a_record_claiming_a_newer_head_needs_the_log_and_is_accepted_after_it_arrives() {
    let mut store = RecordStore::new();
    store.offer_log(log_one()).unwrap();

    let ahead = current(&sk(1), &sk(3), &log_two(), 5);
    match store.put(ahead.clone(), NOW) {
        Err(NameError::NeedSuccessionLog { claimed, held, .. }) => {
            assert_eq!((claimed, held), (2, 1));
        }
        other => panic!("expected NeedSuccessionLog, got {other:?}"),
    }
    assert_eq!(
        store.len(),
        0,
        "nothing was stored while the log is missing"
    );

    assert_eq!(
        store.offer_log(log_two()).unwrap(),
        LogChange::Adopted {
            inception_key: pk(&sk(1))
        }
    );
    assert_eq!(store.put(ahead.clone(), NOW).unwrap(), PutOutcome::Inserted);

    // sk(2) is superseded by entry 2, as the first key is.
    let by_superseded = current(&sk(1), &sk(2), &log_one(), 6);
    assert!(matches!(
        store.put(by_superseded, NOW),
        Err(NameError::SuccessionRefused { .. })
    ));
}

#[test]
fn a_retired_name_refuses_everything_with_a_distinct_error() {
    let mut store = RecordStore::new();
    store.put(plain(&sk(1), 1), NOW).unwrap();
    store.offer_log(log_retired()).unwrap();
    assert_eq!(store.len(), 0, "a retired name keeps no record");

    // Even a record that states the retired log's own head, signed by the key
    // that was current.
    let claim = log_retired()
        .verify(&pk(&sk(1)))
        .unwrap()
        .head_claim()
        .unwrap();
    let by_current = raw_sign(
        record(&pk(&sk(1)), 4).with_meta(SUCCESSION_META_KEY, claim.to_meta_value()),
        &sk(2),
    );
    assert!(matches!(
        store.put(by_current, NOW),
        Err(NameError::Retired(_))
    ));
    assert!(matches!(
        store.put(plain(&sk(1), 9), NOW),
        Err(NameError::Retired(_))
    ));
    // A tombstone is a record too.
    let state = log_one().verify(&pk(&sk(1))).unwrap();
    let mut tomb = NameRecord::tombstone(SpineUri::did(pk(&sk(1))), 10, NOW).unwrap();
    tomb.sign_current(&sk(2), &state).unwrap();
    assert!(matches!(store.put(tomb, NOW), Err(NameError::Retired(_))));
}

#[tokio::test]
async fn a_retired_name_resolves_as_retired_even_from_the_cache() {
    let r = LocalResolver::at_time(NOW);
    let old = plain(&sk(1), 1);
    r.cache_record(old.clone()).unwrap();
    assert!(r.resolve(&old.name).await.is_ok());

    r.offer_log(log_retired()).unwrap();
    assert!(matches!(
        r.resolve(&old.name).await,
        Err(NameError::Retired(_))
    ));
    assert!(matches!(r.cache_record(old), Err(NameError::Retired(_))));
}

#[tokio::test]
async fn a_rotation_drops_what_the_cache_held_for_the_old_key() {
    let r = LocalResolver::at_time(NOW);
    let old = plain(&sk(1), 1);
    r.cache_record(old.clone()).unwrap();
    r.offer_log(log_one()).unwrap();
    assert!(matches!(
        r.resolve(&old.name).await,
        Err(NameError::NotFound(_))
    ));
    assert!(matches!(
        r.cache_record(old),
        Err(NameError::SupersededKey(_))
    ));
    let fresh = current(&sk(1), &sk(2), &log_one(), 2);
    r.publish(fresh.clone()).unwrap();
    assert_eq!(r.resolve(&fresh.name).await.unwrap().record, fresh);
}

#[tokio::test]
async fn equivocation_fails_the_name_closed() {
    let r = LocalResolver::at_time(NOW);
    let held = plain(&sk(1), 1);
    r.publish(held.clone()).unwrap();
    r.offer_log(log_one()).unwrap();
    assert_eq!(
        r.offer_log(log_fork()).unwrap(),
        LogChange::Compromised {
            inception_key: pk(&sk(1))
        }
    );
    let proof = r.equivocation(&pk(&sk(1))).unwrap();
    proof.verify(&pk(&sk(1))).unwrap();
    assert_eq!(proof.index(), 1);

    assert!(matches!(
        r.resolve(&held.name).await,
        Err(NameError::Equivocated(_))
    ));
    // No record is admitted, by any key either log names.
    assert!(matches!(
        r.publish(plain(&sk(1), 5)),
        Err(NameError::Equivocated(_))
    ));
    assert!(matches!(
        r.cache_record(current(&sk(1), &sk(2), &log_one(), 6)),
        Err(NameError::Equivocated(_))
    ));
    // Later entries and either log again do not clear it.
    assert_eq!(r.offer_log(log_two()).unwrap(), LogChange::Unchanged);
    assert_eq!(r.offer_log(log_fork()).unwrap(), LogChange::Unchanged);
    assert!(matches!(
        r.resolve(&held.name).await,
        Err(NameError::Equivocated(_))
    ));
    assert_eq!(r.local_len(), 0);
}

#[tokio::test]
async fn publishing_with_the_log_admits_a_rotated_record_in_one_step() {
    let r = LocalResolver::at_time(NOW);
    let fresh = current(&sk(1), &sk(2), &log_one(), 2);

    // Without the log the head claim cannot be judged: the log must come first.
    assert!(matches!(
        r.publish(fresh.clone()),
        Err(NameError::NeedSuccessionLog { .. })
    ));
    // A log of another name is not taken for this record.
    let (_, foreign) = log_for(3);
    assert!(matches!(
        r.publish_with_log(fresh.clone(), foreign),
        Err(NameError::InvalidSuccessionLog(_))
    ));
    assert!(r.succession_state(&pk(&sk(1))).is_none());

    r.publish_with_log(fresh.clone(), log_one()).unwrap();
    assert_eq!(r.resolve(&fresh.name).await.unwrap().record, fresh);
    assert_eq!(r.succession_log(&pk(&sk(1))), Some(log_one()));
    // An empty log is the unrotated case: plain publication.
    r.publish_with_log(plain(&sk(9), 1), SuccessionLog::default())
        .unwrap();
}

#[cfg(feature = "storage")]
mod persistence {
    use super::*;
    use spine_storage::{InMemoryBackend, StorageBackend};
    use std::sync::Arc;

    fn open(backend: &Arc<InMemoryBackend>, now: u64) -> LocalResolver {
        LocalResolver::persistent(move || now, backend.clone()).unwrap()
    }

    #[tokio::test]
    async fn a_rotation_and_its_records_survive_a_restart() {
        let backend = Arc::new(InMemoryBackend::new());
        let fresh = current(&sk(1), &sk(2), &log_one(), 2);
        {
            let r = open(&backend, NOW);
            r.offer_log(log_one()).unwrap();
            r.publish(fresh.clone()).unwrap();
        }
        let r = open(&backend, NOW + 10);
        assert_eq!(r.succession_log(&pk(&sk(1))), Some(log_one()));
        assert_eq!(r.resolve(&fresh.name).await.unwrap().record, fresh);
        // The old key is still refused after the restart.
        assert!(matches!(
            r.publish(plain(&sk(1), 9)),
            Err(NameError::SupersededKey(_))
        ));
        // And the log extends across a restart, replacing the stored one.
        r.offer_log(log_two()).unwrap();
        drop(r);
        let r = open(&backend, NOW + 20);
        assert_eq!(r.succession_log(&pk(&sk(1))), Some(log_two()));
    }

    #[tokio::test]
    async fn equivocation_stays_failed_closed_after_a_restart() {
        let backend = Arc::new(InMemoryBackend::new());
        {
            let r = open(&backend, NOW);
            r.offer_log(log_one()).unwrap();
            r.offer_log(log_fork()).unwrap();
        }
        assert_eq!(
            backend
                .count(spine_name::resolver::EQUIVOCATIONS_NAMESPACE)
                .unwrap(),
            1
        );
        // The log row is gone: the proof carries what the name needs.
        assert_eq!(
            backend
                .count(spine_name::resolver::SUCCESSIONS_NAMESPACE)
                .unwrap(),
            0
        );
        let r = open(&backend, NOW + 10);
        let name = SpineUri::did(pk(&sk(1)));
        assert!(matches!(
            r.resolve(&name).await,
            Err(NameError::Equivocated(_))
        ));
        assert!(matches!(
            r.publish(plain(&sk(1), 3)),
            Err(NameError::Equivocated(_))
        ));
        assert!(r.equivocation(&pk(&sk(1))).is_some());
    }

    #[test]
    fn an_invalid_stored_log_or_proof_is_dropped_at_load() {
        let backend = Arc::new(InMemoryBackend::new());
        // A log stored under the wrong key, one that is not JSON, one with a bad
        // signature, a key of the wrong length, and a proof that proves nothing.
        let wrong_key = serde_json::to_vec(&log_one()).unwrap();
        backend
            .put(
                spine_name::resolver::SUCCESSIONS_NAMESPACE,
                &pk(&sk(9)),
                &wrong_key,
            )
            .unwrap();
        backend
            .put(
                spine_name::resolver::SUCCESSIONS_NAMESPACE,
                &pk(&sk(11)),
                b"not json",
            )
            .unwrap();
        let mut forged = log_one();
        forged.entries[0].signature[0] ^= 1;
        backend
            .put(
                spine_name::resolver::SUCCESSIONS_NAMESPACE,
                &pk(&sk(1)),
                &serde_json::to_vec(&forged).unwrap(),
            )
            .unwrap();
        backend
            .put(
                spine_name::resolver::SUCCESSIONS_NAMESPACE,
                b"short",
                &wrong_key,
            )
            .unwrap();
        let not_a_proof = spine_name::Equivocation {
            prefix: SuccessionLog::default(),
            first: log_one().entries[0].clone(),
            second: log_one().entries[0].clone(),
        };
        backend
            .put(
                spine_name::resolver::EQUIVOCATIONS_NAMESPACE,
                &pk(&sk(1)),
                &serde_json::to_vec(&not_a_proof).unwrap(),
            )
            .unwrap();
        let r = open(&backend, NOW);
        assert!(r.succession_state(&pk(&sk(1))).is_none());
        assert!(r.succession_state(&pk(&sk(9))).is_none());
        assert!(r.equivocation(&pk(&sk(1))).is_none());
        // The name behaves as a name with no log.
        r.publish(plain(&sk(1), 1)).unwrap();
    }
}

fn key_n(i: usize) -> SigningKey {
    let mut seed = [7u8; 32];
    seed[..8].copy_from_slice(&(i as u64).to_be_bytes());
    SigningKey::from_bytes(&seed)
}

/// A one-entry log for the name with inception key `key_n(i)`.
fn log_for(i: usize) -> (SigningKey, SuccessionLog) {
    let inception = key_n(i);
    let log = SuccessionLog::default()
        .rotated(&pk(&inception), &inception, &pk(&sk(2)), &pk(&sk(3)), 1_000)
        .unwrap();
    (inception, log)
}

fn adopt(book: &mut LogBook, key: &[u8; 32], log: SuccessionLog) -> LogOffer {
    let offer = book.plan(key, log, &HashSet::new()).unwrap();
    book.apply(offer.clone());
    offer
}

#[test]
fn a_full_log_table_never_evicts_a_log_whose_records_are_held() {
    let mut book = LogBook::new();
    let mut keys = Vec::new();
    for i in 0..MAX_SUCCESSION_LOGS {
        let (inception, log) = log_for(i);
        let key = pk(&inception);
        adopt(&mut book, &key, log);
        keys.push(key);
    }
    assert!(book.is_full());

    let (extra, extra_log) = log_for(MAX_SUCCESSION_LOGS);
    let extra_key = pk(&extra);

    // Every name has a record held: nothing may go, so the newcomer is refused.
    let all: HashSet<[u8; 32]> = keys.iter().copied().collect();
    assert_eq!(
        book.plan(&extra_key, extra_log.clone(), &all).unwrap(),
        LogOffer::Full
    );

    // One name without records held is the one given up.
    let mut protected = all.clone();
    protected.remove(&keys[17]);
    match book.plan(&extra_key, extra_log, &protected).unwrap() {
        LogOffer::Adopt { evict, .. } => assert_eq!(evict, Some(keys[17])),
        other => panic!("expected Adopt, got {other:?}"),
    }
    assert!(book.state(&keys[17]).is_some(), "planning evicts nothing");
}

#[test]
fn a_retired_log_is_evicted_after_a_live_one() {
    let mut book = LogBook::new();
    let (first, first_log) = log_for(0);
    let retired_log = first_log
        .retired(&pk(&first), &sk(2), &pk(&sk(3)), 1_001)
        .unwrap();
    // The retired name is the oldest; a live one younger than it goes first.
    adopt(&mut book, &pk(&first), retired_log);
    let mut live = Vec::new();
    for i in 1..MAX_SUCCESSION_LOGS {
        let (inception, log) = log_for(i);
        adopt(&mut book, &pk(&inception), log);
        live.push(pk(&inception));
    }
    let (extra, extra_log) = log_for(MAX_SUCCESSION_LOGS);
    match book.plan(&pk(&extra), extra_log, &HashSet::new()).unwrap() {
        LogOffer::Adopt { evict, .. } => assert_eq!(evict, Some(live[0])),
        other => panic!("expected Adopt, got {other:?}"),
    }
}

#[test]
fn a_full_proof_table_refuses_a_conflicting_log_and_keeps_the_held_one() {
    let mut book = LogBook::new();
    for i in 0..MAX_EQUIVOCATION_PROOFS {
        let (inception, log) = log_for(i);
        let key = pk(&inception);
        adopt(&mut book, &key, log);
        let fork = SuccessionLog::default()
            .rotated(&key, &inception, &pk(&sk(7)), &pk(&sk(8)), 1_000)
            .unwrap();
        let offer = book.plan(&key, fork, &HashSet::new()).unwrap();
        assert!(matches!(offer, LogOffer::Conflict(_)));
        book.apply(offer);
    }
    assert_eq!(book.proof_count(), MAX_EQUIVOCATION_PROOFS);

    let (inception, log) = log_for(MAX_EQUIVOCATION_PROOFS);
    let key = pk(&inception);
    adopt(&mut book, &key, log.clone());
    let fork = SuccessionLog::default()
        .rotated(&key, &inception, &pk(&sk(7)), &pk(&sk(8)), 1_000)
        .unwrap();
    assert_eq!(
        book.plan(&key, fork, &HashSet::new()).unwrap(),
        LogOffer::ProofsFull
    );
    // The name keeps the log it had and does not fail closed here.
    assert_eq!(book.log(&key), Some(&log));
    assert!(!book.is_compromised(&key));
}

#[test]
fn signing_with_the_current_key() {
    let state0 = log_one().verify(&pk(&sk(1))).unwrap();
    let empty = SuccessionLog::default().verify(&pk(&sk(1))).unwrap();

    // An unrotated name: `sign_current` is `sign`, with no claim.
    let mut a = record(&pk(&sk(1)), 1);
    a.sign_current(&sk(1), &empty).unwrap();
    let mut b = record(&pk(&sk(1)), 1);
    b.sign(&sk(1)).unwrap();
    assert_eq!(a, b);
    assert!(!a.meta.contains_key(SUCCESSION_META_KEY));

    // A rotated name: only the current key signs, and the claim is the head.
    let mut c = record(&pk(&sk(1)), 2);
    assert_eq!(c.sign_current(&sk(1), &state0), Err(NameError::KeyMismatch));
    assert_eq!(c.sign_current(&sk(5), &state0), Err(NameError::KeyMismatch));
    c.sign_current(&sk(2), &state0).unwrap();
    assert_eq!(
        c.meta.get(SUCCESSION_META_KEY).unwrap(),
        &state0.head_claim().unwrap().to_meta_value()
    );
    // `sign` for the same key still refuses: it only takes the name key.
    assert_eq!(c.sign(&sk(2)), Err(NameError::KeyMismatch));

    // A name that is not the log's, and a retired name.
    let mut other = record(&pk(&sk(9)), 1);
    assert!(matches!(
        other.sign_current(&sk(2), &state0),
        Err(NameError::SuccessionRefused { .. })
    ));
    let retired = log_retired().verify(&pk(&sk(1))).unwrap();
    let mut d = record(&pk(&sk(1)), 3);
    assert!(matches!(
        d.sign_current(&sk(2), &retired),
        Err(NameError::Retired(_))
    ));

    // A tombstone carries no claim and is admitted under the log.
    let mut store = RecordStore::new();
    store.offer_log(log_one()).unwrap();
    let mut tomb = NameRecord::tombstone(SpineUri::did(pk(&sk(1))), 7, NOW).unwrap();
    tomb.sign_current(&sk(2), &state0).unwrap();
    assert!(!tomb.meta.contains_key(SUCCESSION_META_KEY));
    assert_eq!(store.put(tomb, NOW).unwrap(), PutOutcome::Inserted);
}

#[test]
fn renewing_a_rotated_name_signs_with_the_current_key_and_the_newest_claim() {
    let old = current(&sk(1), &sk(2), &log_one(), 4);
    let state2 = log_two().verify(&pk(&sk(1))).unwrap();
    let renewed = old.renewed_current(&sk(3), &state2, 1_050, 200).unwrap();
    assert_eq!(renewed.seq, 5);
    assert_eq!(renewed.published_at, 1_050);
    assert_eq!(
        renewed.meta.get(SUCCESSION_META_KEY).unwrap(),
        &state2.head_claim().unwrap().to_meta_value()
    );
    let mut store = RecordStore::new();
    store.offer_log(log_two()).unwrap();
    store.put(renewed, 1_050).unwrap();
    // The superseded key cannot renew.
    assert_eq!(
        old.renewed_current(&sk(2), &state2, 1_050, 200),
        Err(NameError::KeyMismatch)
    );
    // A tombstone is never renewed.
    let mut tomb = NameRecord::tombstone(SpineUri::did(pk(&sk(1))), 7, NOW).unwrap();
    tomb.sign_current(&sk(3), &state2).unwrap();
    assert!(matches!(
        tomb.renewed_current(&sk(3), &state2, 1_050, 200),
        Err(NameError::Revoked(_))
    ));
}

#[test]
fn a_publication_carries_the_log_beside_a_record_a_plain_reader_still_parses() {
    let rec = current(&sk(1), &sk(2), &log_one(), 2);
    let value = publication_value(&rec, &log_one()).unwrap();
    let as_record: NameRecord = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(as_record, rec);
    assert_eq!(publication_log(&value).unwrap().unwrap(), log_one());
    assert!(publication_log(&serde_json::to_value(&rec).unwrap()).is_none());
}

#[test]
fn logs_over_the_bounds_are_recognised_before_any_verification() {
    assert!(log_within_bounds(&log_two()));
    let mut long = log_one();
    while long.entries.len() <= MAX_SUCCESSION_ENTRIES {
        long.entries.push(long.entries[0].clone());
    }
    assert!(!log_within_bounds(&long));
    let mut short_sig = log_one();
    short_sig.entries[0].signature.truncate(63);
    assert!(!log_within_bounds(&short_sig));
}
