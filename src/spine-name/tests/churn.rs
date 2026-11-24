//! The record store and the persistent resolver under conflict, pressure and
//! restart (spec 02, sections 3 to 6): the ordering of records of one name is
//! total and arrival-order independent, a store at capacity gives up the record
//! furthest from home, expiry hides a record before any sweep removes it, a
//! tombstone outlives what it revokes for its own stated lifetime, and what a
//! publisher persisted survives a reopen with its sequence numbers intact.
//!
//! Time is always a parameter here: nothing sleeps.

use ed25519_dalek::SigningKey;
use spine_name::{
    Endpoint, NameKey, NameRecord, PutOutcome, RecordStore, SpineUri, MAX_RECORD_TTL_SECS,
};

const NOW: u64 = 10_000;

fn sk(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn name_of(seed: u8) -> SpineUri {
    SpineUri::did(sk(seed).verifying_key().to_bytes())
}

/// A signed record of the name derived from `seed`, distinguished by `endpoint`.
fn rec(seed: u8, seq: u64, at: u64, endpoint: &str) -> NameRecord {
    let key = sk(seed);
    let mut r = NameRecord::new(name_of(seed), seq, at)
        .unwrap()
        .with_endpoint(Endpoint::new("tcp", endpoint));
    r.sign(&key).unwrap();
    r
}

fn rec_ttl(seed: u8, seq: u64, at: u64, ttl: u32) -> NameRecord {
    let key = sk(seed);
    let mut r = NameRecord::new(name_of(seed), seq, at)
        .unwrap()
        .with_ttl(ttl)
        .with_endpoint(Endpoint::new("tcp", "h:1"));
    r.sign(&key).unwrap();
    r
}

fn tomb(seed: u8, seq: u64, at: u64) -> NameRecord {
    let key = sk(seed);
    let mut r = NameRecord::tombstone(name_of(seed), seq, at).unwrap();
    r.sign(&key).unwrap();
    r
}

/// Two fresh stores that are offered `a` and `b` in opposite orders.
fn both_orders(a: &NameRecord, b: &NameRecord) -> (RecordStore, RecordStore) {
    let mut ab = RecordStore::new();
    ab.put(a.clone(), NOW).unwrap();
    ab.put(b.clone(), NOW).unwrap();
    let mut ba = RecordStore::new();
    ba.put(b.clone(), NOW).unwrap();
    ba.put(a.clone(), NOW).unwrap();
    (ab, ba)
}

// ---- 1. conflicting sequences ------------------------------------------------

#[test]
fn equal_seq_with_different_publication_times_keeps_the_later_in_either_order() {
    let early = rec(1, 5, NOW - 100, "early:1");
    let late = rec(1, 5, NOW - 10, "late:1");
    let (ab, ba) = both_orders(&early, &late);
    let name = early.name.clone();
    assert_eq!(ab.get(&name), Some(&late));
    assert_eq!(ba.get(&name), Some(&late));
    assert_eq!(ab.len(), 1);
    assert_eq!(ba.len(), 1);
}

#[test]
fn equal_seq_and_time_with_different_content_converges_on_one_record() {
    let a = rec(1, 5, NOW - 10, "a:1");
    let b = rec(1, 5, NOW - 10, "b:1");
    assert_ne!(
        a, b,
        "the two records must be distinct for this to mean anything"
    );
    assert_ne!(a.signature, b.signature);

    let (ab, ba) = both_orders(&a, &b);
    let name = a.name.clone();
    // The documented tie-break is the greater signature (record.rs, supersedes).
    let winner = if a.signature > b.signature { &a } else { &b };
    assert_eq!(ab.get(&name), Some(winner));
    assert_eq!(ba.get(&name), Some(winner));
}

#[test]
fn the_held_winner_is_not_replaced_by_itself_or_by_the_loser() {
    let a = rec(1, 5, NOW - 10, "a:1");
    let b = rec(1, 5, NOW - 10, "b:1");
    let (winner, loser) = if a.signature > b.signature {
        (a, b)
    } else {
        (b, a)
    };
    let mut s = RecordStore::new();
    s.put(winner.clone(), NOW).unwrap();
    assert_eq!(s.put(winner.clone(), NOW).unwrap(), PutOutcome::Superseded);
    assert_eq!(s.put(loser, NOW).unwrap(), PutOutcome::Superseded);
    assert_eq!(s.get(&winner.name), Some(&winner));
}

#[test]
fn a_higher_seq_beats_a_later_publication_in_either_order() {
    let newer_seq = rec(1, 6, NOW - 500, "seq6:1");
    let newer_time = rec(1, 5, NOW - 1, "seq5:1");
    let (ab, ba) = both_orders(&newer_seq, &newer_time);
    let name = newer_seq.name.clone();
    assert_eq!(ab.get(&name), Some(&newer_seq));
    assert_eq!(ba.get(&name), Some(&newer_seq));
}

#[test]
fn equal_seq_tombstones_keep_the_later_in_either_order() {
    let first = tomb(1, 5, NOW - 100);
    let second = tomb(1, 5, NOW - 10);
    let (ab, ba) = both_orders(&first, &second);
    let name = first.name.clone();
    assert_eq!(ab.get(&name), Some(&second));
    assert_eq!(ba.get(&name), Some(&second));
}

// ---- 6. convergence over every delivery order --------------------------------

/// Offer `records` (all of one name) to fresh stores in many orders and check
/// that every store ends holding `expected`.
fn assert_every_order_converges(records: &[NameRecord], expected: &NameRecord) {
    let n = records.len();
    let mut orders: Vec<Vec<usize>> = vec![(0..n).collect(), (0..n).rev().collect()];
    // A fixed-seed generator, so a failure reproduces.
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..60 {
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let j = ((state >> 33) as usize) % (i + 1);
            order.swap(i, j);
        }
        orders.push(order);
    }
    for order in orders {
        let mut s = RecordStore::new();
        for &i in &order {
            s.put(records[i].clone(), NOW).unwrap();
        }
        assert_eq!(s.len(), 1, "order {order:?}");
        assert_eq!(s.get(&expected.name), Some(expected), "order {order:?}");
    }
}

#[test]
fn every_delivery_order_ends_with_the_same_record_when_the_winner_is_a_record() {
    let records = vec![
        rec(1, 1, NOW - 1_000, "a:1"),
        rec(1, 2, NOW - 500, "b:1"),
        tomb(1, 3, NOW - 1_000),
        rec(1, 3, NOW - 100, "c:1"),
        rec(1, 4, NOW - 900, "d:1"),
        rec(1, 4, NOW - 900, "e:1"),
    ];
    // seq 4 beats everything below it; the last two tie on seq and time, so the
    // greater signature decides.
    let expected = if records[4].signature > records[5].signature {
        records[4].clone()
    } else {
        records[5].clone()
    };
    assert_every_order_converges(&records, &expected);
}

#[test]
fn every_delivery_order_ends_with_the_same_record_when_the_winner_is_a_tombstone() {
    let records = vec![
        rec(2, 5, NOW - 100, "a:1"),
        tomb(2, 5, NOW - 1_000),
        tomb(2, 5, NOW - 800),
        rec(2, 5, NOW - 50, "b:1"),
        rec(2, 2, NOW - 10, "c:1"),
        tomb(2, 1, NOW - 10),
    ];
    // At seq 5 a tombstone beats any record whatever the times, and of the two
    // tombstones the later publication wins.
    let expected = records[2].clone();
    assert!(expected.is_tombstone());
    assert_every_order_converges(&records, &expected);
}

// ---- 4. eviction, expiry and tombstone lifetime ------------------------------

/// The seeds `1..=12` ordered nearest first by distance of their name key from
/// `home`, using the library's own distance.
fn by_distance(home: &NameKey) -> Vec<u8> {
    let mut seeds: Vec<u8> = (1..=12).collect();
    seeds.sort_by_key(|s| home.distance(&name_of(*s).key()));
    seeds
}

#[test]
fn a_full_store_evicts_the_record_furthest_from_home() {
    let home = NameKey::of(b"churn-home");
    let order = by_distance(&home);
    let mut s = RecordStore::with_limits(home, 3);
    assert_eq!(s.capacity(), 3);

    for &seed in &order[5..8] {
        assert_eq!(
            s.put(rec(seed, 1, NOW, "h:1"), NOW).unwrap(),
            PutOutcome::Inserted
        );
    }
    // Nearer than everything held: admitted, and the furthest held goes.
    assert_eq!(
        s.put(rec(order[0], 1, NOW, "h:1"), NOW).unwrap(),
        PutOutcome::Inserted
    );
    assert_eq!(s.len(), 3);
    assert!(s.get(&name_of(order[7])).is_none(), "furthest was evicted");
    assert!(s.get(&name_of(order[0])).is_some());

    // Further than everything held: refused, nothing displaced.
    assert_eq!(
        s.put(rec(order[11], 1, NOW, "h:1"), NOW).unwrap(),
        PutOutcome::Rejected
    );
    assert_eq!(s.len(), 3);
    assert!(s.get(&name_of(order[11])).is_none());

    // Nearer than the furthest now held (order[6]): admitted, that one goes.
    assert_eq!(
        s.put(rec(order[1], 1, NOW, "h:1"), NOW).unwrap(),
        PutOutcome::Inserted
    );
    assert!(s.get(&name_of(order[6])).is_none());
    for kept in [order[0], order[1], order[5]] {
        assert!(s.get(&name_of(kept)).is_some(), "seed {kept} should remain");
    }
}

#[test]
fn replacing_a_held_name_at_capacity_evicts_nothing() {
    let home = NameKey::of(b"churn-home");
    let order = by_distance(&home);
    let mut s = RecordStore::with_limits(home, 2);
    s.put(rec(order[3], 1, NOW, "h:1"), NOW).unwrap();
    s.put(rec(order[9], 1, NOW, "h:1"), NOW).unwrap();
    // The furthest name is updated in place, not evicted to make room for itself.
    assert_eq!(
        s.put(rec(order[9], 2, NOW, "h:2"), NOW).unwrap(),
        PutOutcome::Updated
    );
    assert_eq!(s.len(), 2);
    assert!(s.get(&name_of(order[3])).is_some());
    assert_eq!(s.get(&name_of(order[9])).unwrap().seq, 2);
}

/// Documented in `RecordStore::put` (decision 52): a live record is evicted
/// before a tombstone, so a nearer arrival does not cost the furthest tombstone
/// its place.
#[test]
fn a_full_store_of_live_records_and_a_tombstone_keeps_the_tombstone() {
    let home = NameKey::of(b"churn-home");
    let order = by_distance(&home);
    let (near, mid, far) = (order[0], order[5], order[11]);
    let mut s = RecordStore::with_limits(home, 2);
    s.put(tomb(far, 2, NOW), NOW).unwrap();
    s.put(rec(mid, 1, NOW, "h:1"), NOW).unwrap();
    assert!(s.is_revoked(&name_of(far), NOW));
    assert_eq!(
        s.put(rec(far, 1, NOW, "h:1"), NOW).unwrap(),
        PutOutcome::Superseded,
        "while held, the tombstone blocks the old record"
    );

    // A nearer record arrives: the live record goes, though the tombstone is
    // the furthest.
    assert_eq!(
        s.put(rec(near, 1, NOW, "h:1"), NOW).unwrap(),
        PutOutcome::Inserted
    );
    assert!(s.is_revoked(&name_of(far), NOW));
    assert!(s.get(&name_of(mid)).is_none());
    assert_eq!(
        s.put(rec(far, 1, NOW, "h:1"), NOW).unwrap(),
        PutOutcome::Superseded
    );
}

/// A tombstone flood cannot exceed its share of capacity and cannot push the
/// live records out.
#[test]
fn a_tombstone_flood_cannot_exceed_its_share_or_evict_live_records() {
    let home = NameKey::of(b"churn-home");
    let order = by_distance(&home);
    let mut s = RecordStore::with_limits(home, 6);
    assert_eq!(s.tombstone_limit(), 3);
    for &seed in &order[0..3] {
        s.put(rec(seed, 1, NOW, "h:1"), NOW).unwrap();
    }
    for (i, &seed) in order[3..12].iter().enumerate() {
        s.put(tomb(seed, 2, NOW - 100 + i as u64), NOW).unwrap();
        assert!(s.tombstone_count() <= 3);
        assert!(s.len() <= 6);
    }
    for &seed in &order[0..3] {
        assert!(s.get(&name_of(seed)).is_some(), "live seed {seed} stayed");
    }
    // The three newest tombstones are the ones held.
    for &seed in &order[9..12] {
        assert!(s.is_revoked(&name_of(seed), NOW));
    }
    assert_eq!(s.tombstone_count(), 3);
}

/// What the share does not do: a tombstone pushed out by newer tombstones stops
/// blocking the record it revoked, so the name can be resurrected. Only a
/// `retire` entry of a succession log is permanent.
#[test]
fn a_tombstone_evicted_over_the_share_stops_blocking_the_record_it_revoked() {
    let home = NameKey::of(b"churn-home");
    let order = by_distance(&home);
    let (mid, far) = (order[5], order[11]);
    let mut s = RecordStore::with_limits(home, 2);
    assert_eq!(s.tombstone_limit(), 1);
    s.put(tomb(far, 2, NOW - 10), NOW).unwrap();
    assert_eq!(
        s.put(rec(far, 1, NOW, "h:1"), NOW).unwrap(),
        PutOutcome::Superseded,
        "while held, the tombstone blocks the old record"
    );

    // A newer tombstone of another name takes the only tombstone place.
    s.put(tomb(mid, 2, NOW), NOW).unwrap();
    assert!(s.get(&name_of(far)).is_none());
    assert!(!s.is_revoked(&name_of(far), NOW));
    assert!(s.is_revoked(&name_of(mid), NOW));

    // The revoked record is admitted again.
    assert_eq!(
        s.put(rec(far, 1, NOW, "h:1"), NOW).unwrap(),
        PutOutcome::Inserted
    );
}

#[test]
fn an_expired_record_is_hidden_before_a_sweep_and_gone_after() {
    let mut s = RecordStore::new();
    let mut short = NameRecord::new(name_of(1), 1, NOW)
        .unwrap()
        .with_ttl(100)
        .with_capability("web.search");
    short.sign(&sk(1)).unwrap();
    let long = rec_ttl(2, 1, NOW, 5_000);
    s.put(short.clone(), NOW).unwrap();
    s.put(long.clone(), NOW).unwrap();

    // One second before the expiry instant it is fresh; at the instant it is not.
    assert!(s.get_fresh(&short.name, NOW + 99).is_some());
    assert!(s.get_fresh(&short.name, NOW + 100).is_none());
    assert!(s.providers_of("web.search", NOW + 100).is_empty());
    // Still held, so still counted, until swept.
    assert!(s.get(&short.name).is_some());
    assert_eq!(s.len(), 2);

    assert_eq!(s.sweep_expired(NOW + 100), 1);
    assert_eq!(s.len(), 1);
    assert!(s.get(&short.name).is_none());
    assert!(s.capability_terms().is_empty());
    assert!(s.get_fresh(&long.name, NOW + 100).is_some());
    assert_eq!(
        s.sweep_expired(NOW + 100),
        0,
        "a second sweep finds nothing"
    );
}

#[test]
fn a_tombstone_outlives_the_record_it_revokes_for_its_own_lifetime() {
    let max = u64::from(MAX_RECORD_TTL_SECS);
    let old = rec_ttl(1, 1, NOW, 3_600);
    let t = tomb(1, 2, NOW);
    let name = old.name.clone();

    let mut s = RecordStore::new();
    s.put(old.clone(), NOW).unwrap();
    assert_eq!(s.put(t.clone(), NOW).unwrap(), PutOutcome::Updated);

    // Long after the old record lapsed, the tombstone still revokes the name and
    // refuses a replay of it and a record of the same seq.
    let later = NOW + 3_600 * 24;
    assert!(old.is_expired(later));
    assert!(s.is_revoked(&name, later));
    assert_eq!(s.put(old.clone(), later).unwrap(), PutOutcome::Superseded);
    assert_eq!(
        s.put(rec(1, 2, later, "same-seq:1"), later).unwrap(),
        PutOutcome::Superseded
    );
    assert!(s.is_revoked(&name, NOW + max - 1));

    // At the end of its lifetime it no longer revokes: the name is unknown, not
    // revoked, though the lapsed tombstone is still held until a sweep.
    assert!(!s.is_revoked(&name, NOW + max));
    assert!(s.get_fresh(&name, NOW + max).is_none());
    assert!(s.get(&name).is_some());
    assert_eq!(s.sweep_expired(NOW + max), 1);
    assert!(s.is_empty());

    // Swept, the expired old record can be offered again; it is held but, being
    // long expired, is never served.
    assert_eq!(s.put(old, NOW + max).unwrap(), PutOutcome::Inserted);
    assert!(s.get_fresh(&name, NOW + max).is_none());
}

#[test]
fn a_tombstone_blocks_equal_and_lower_seq_in_any_arrival_order_but_yields_to_a_higher_one() {
    let t = tomb(1, 4, NOW);
    for order in [[0usize, 1, 2], [2, 1, 0], [1, 2, 0]] {
        let inputs = [rec(1, 1, NOW, "a:1"), rec(1, 4, NOW, "b:1"), t.clone()];
        let mut s = RecordStore::new();
        for i in order {
            s.put(inputs[i].clone(), NOW).unwrap();
        }
        assert!(s.get(&t.name).unwrap().is_tombstone(), "order {order:?}");
        assert_eq!(
            s.put(rec(1, 5, NOW, "c:1"), NOW).unwrap(),
            PutOutcome::Updated
        );
        assert!(!s.is_revoked(&t.name, NOW));
    }
}

// ---- 5. restart --------------------------------------------------------------

#[cfg(feature = "storage")]
mod restart {
    use super::*;
    use spine_name::{LocalResolver, NameError, Resolver};
    use spine_storage::InMemoryBackend;
    use std::sync::Arc;

    fn open(backend: &Arc<InMemoryBackend>, now: u64) -> LocalResolver {
        LocalResolver::persistent(move || now, backend.clone()).unwrap()
    }

    fn long(seed: u8, seq: u64, at: u64, endpoint: &str) -> NameRecord {
        let key = sk(seed);
        let mut r = NameRecord::new(name_of(seed), seq, at)
            .unwrap()
            .with_ttl(100_000)
            .with_endpoint(Endpoint::new("tcp", endpoint));
        r.sign(&key).unwrap();
        r
    }

    #[tokio::test]
    async fn a_reopened_resolver_holds_the_same_records_and_refuses_a_stale_seq() {
        let backend = Arc::new(InMemoryBackend::new());
        let a = long(1, 3, NOW, "a:1");
        let b = long(2, 1, NOW, "b:1");
        let c_old = long(3, 2, NOW, "c:1");
        let c_tomb = tomb(3, 4, NOW);
        {
            let r = open(&backend, NOW);
            r.publish(a.clone()).unwrap();
            r.publish(b.clone()).unwrap();
            r.publish(c_old.clone()).unwrap();
            r.publish(c_tomb.clone()).unwrap();
        }

        let r = open(&backend, NOW + 10);
        assert_eq!(r.resolve(&a.name).await.unwrap().record, a);
        assert_eq!(r.resolve(&b.name).await.unwrap().record, b);
        assert!(matches!(
            r.resolve(&c_tomb.name).await,
            Err(NameError::Revoked(_))
        ));
        assert_eq!(r.local_len(), 3);
        assert_eq!(r.published_records().unwrap().len(), 3);

        // A lower seq is refused, an equal seq published earlier is too, and the
        // tombstone still blocks lower and equal seq of its name.
        assert_eq!(
            r.publish(long(1, 2, NOW + 10, "stale:1")),
            Err(NameError::StalePublication)
        );
        assert_eq!(
            r.publish(long(1, 3, NOW - 5, "rival:1")),
            Err(NameError::StalePublication)
        );
        r.publish(a.clone()).unwrap();
        for seq in [1, 2, 4] {
            assert_eq!(
                r.publish(long(3, seq, NOW + 10, "replay:1")),
                Err(NameError::StalePublication),
                "seq {seq} against the held tombstone"
            );
        }
        assert_eq!(r.resolve(&a.name).await.unwrap().record, a);

        // A higher seq is accepted and is what the next reopen sees.
        let newer = long(1, 4, NOW + 10, "newer:1");
        r.publish(newer.clone()).unwrap();
        drop(r);
        let r = open(&backend, NOW + 20);
        assert_eq!(r.resolve(&a.name).await.unwrap().record, newer);
        assert_eq!(
            r.publish(a.clone()),
            Err(NameError::StalePublication),
            "the superseded record cannot come back after another restart"
        );
    }

    #[tokio::test]
    async fn a_lapsed_tombstone_still_holds_its_sequence_across_a_restart() {
        let backend = Arc::new(InMemoryBackend::new());
        let t = tomb(1, 4, NOW);
        {
            let r = open(&backend, NOW);
            r.publish(long(1, 2, NOW, "a:1")).unwrap();
            r.publish(t.clone()).unwrap();
        }
        let later = NOW + u64::from(MAX_RECORD_TTL_SECS) + 10;
        let r = open(&backend, later);
        // Lapsed: the name is unknown, not revoked.
        assert!(matches!(
            r.resolve(&t.name).await,
            Err(NameError::NotFound(_))
        ));
        assert_eq!(r.local_len(), 0);
        // But the publisher's sequence floor survives in the backend.
        assert_eq!(
            r.publish(long(1, 3, later, "lower:1")),
            Err(NameError::StalePublication)
        );
        let reborn = long(1, 5, later, "reborn:1");
        r.publish(reborn.clone()).unwrap();
        assert_eq!(r.resolve(&reborn.name).await.unwrap().record, reborn);
    }
}
