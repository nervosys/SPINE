//! The in-memory usage ledger: one charge is atomic across the chain and the
//! nonce, the rolling window follows the time passed in, memory is bounded,
//! and eviction never lets a replay inside the freshness window through.

mod common;

use common::*;
use spine_authority::{
    verify, Chain, ChargeError, Decision, Denial, Effect, Grant, GrantId, LedgerConfig, Limits,
    MemoryLedger, RevocationSet, UsageLedger, RATE_WINDOW_SECS,
};

/// Verifies the standard invocation (nonce byte `n`) against `ledger`.
fn accept(w: &mut World, n: u8, ledger: &MemoryLedger) -> Result<Decision, Denial> {
    w.inv.nonce = [n; 16];
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let c = ctx(w, &roots);
    verify(&Chain::new(grants), &inv, &c, &w.revocations(), ledger)
}

fn ids(grants: &[Grant]) -> Vec<GrantId> {
    grants.iter().map(|g| g.id().unwrap()).collect()
}

/// A decision that charges no grant, only a nonce.
fn nonce_only(n: u8, now: u64, horizon: u64) -> Decision {
    Decision {
        chain_ids: Vec::new(),
        chain_limits: Vec::new(),
        chain_expiries: Vec::new(),
        nonce_horizon: horizon,
        invoker: [0x33u8; 32],
        nonce: [n; 16],
        now,
        effect: Effect::Read,
        cost: 0,
        bytes: 0,
    }
}

/// A decision that charges one grant with the given limits.
fn one_grant(id: GrantId, limits: Limits, expires: u64, n: u8, now: u64) -> Decision {
    Decision {
        chain_ids: vec![id],
        chain_limits: vec![limits],
        chain_expiries: vec![expires],
        nonce_horizon: now + 60,
        invoker: [0x33u8; 32],
        nonce: [n; 16],
        now,
        effect: Effect::Read,
        cost: 1,
        bytes: 1,
    }
}

fn calls(n: u64) -> Limits {
    Limits {
        calls: Some(n),
        ..Limits::default()
    }
}

#[test]
fn a_charge_records_the_nonce_and_every_grant_of_the_chain() {
    let mut w = World::new();
    let grants = w.grants();
    let id = ids(&grants);
    let mut ledger = MemoryLedger::new();
    let decision = accept(&mut w, 1, &ledger).unwrap();
    assert_eq!(decision.chain_ids, id);
    assert_eq!(decision.chain_limits.len(), 3);
    assert_eq!(decision.chain_expiries, [10_000u64, 9_000, 8_000]);
    assert_eq!(decision.nonce_horizon, NOW + 60);

    ledger.charge(&decision).unwrap();
    for grant_id in &id {
        let spent = ledger.spent(grant_id, NOW).unwrap();
        assert_eq!(spent.calls, 1);
        assert_eq!(spent.cost, 1);
        assert_eq!(spent.bytes, 10);
        assert_eq!(spent.recent_calls, 1);
    }
    assert!(ledger.nonce_seen(&decision.invoker, &decision.nonce));
    assert_eq!(ledger.nonce_count(), 1);
    assert_eq!(ledger.grant_count(), 3);
}

#[test]
fn a_replayed_nonce_is_refused_and_changes_nothing() {
    let mut w = World::new();
    let id = ids(&w.grants());
    let mut ledger = MemoryLedger::new();
    let decision = accept(&mut w, 1, &ledger).unwrap();
    ledger.charge(&decision).unwrap();

    assert_eq!(accept(&mut w, 1, &ledger), Err(Denial::NonceReplayed));
    assert_eq!(ledger.charge(&decision), Err(ChargeError::NonceReplayed));
    assert_eq!(ledger.spent(&id[0], NOW).unwrap().calls, 1);
}

#[test]
fn a_chain_whose_third_grant_is_exhausted_charges_nothing() {
    let mut w = World::new();
    let id = ids(&w.grants());
    let mut ledger = MemoryLedger::new();
    // Verification passes while the ledger is empty ...
    let decision = accept(&mut w, 1, &ledger).unwrap();
    // ... and the last grant is used up before the charge arrives.
    ledger.preload(id[2], 10, 0, 0);
    assert_eq!(ledger.charge(&decision), Err(ChargeError::LimitExhausted));

    // The first two grants were not charged, and the nonce was not recorded.
    assert_eq!(ledger.spent(&id[0], NOW).unwrap().calls, 0);
    assert_eq!(ledger.spent(&id[1], NOW).unwrap().calls, 0);
    assert_eq!(ledger.spent(&id[2], NOW).unwrap().calls, 10);
    assert!(!ledger.nonce_seen(&decision.invoker, &decision.nonce));
    assert_eq!(ledger.nonce_count(), 0);
    assert_eq!(ledger.grant_count(), 1);
}

#[test]
fn each_kind_of_limit_in_any_grant_blocks_the_whole_charge() {
    // (grant index, calls, cost, bytes) already used.
    let cases = [
        (0usize, 100u64, 0u64, 0u64),
        (0, 0, 1_000, 0),
        (0, 0, 0, 1_000_000),
        (1, 50, 0, 0),
        (1, 0, 500, 0),
        (2, 0, 100, 0),
        (2, 0, 0, 10_000),
    ];
    for (index, used_calls, used_cost, used_bytes) in cases {
        let mut w = World::new();
        let id = ids(&w.grants());
        let mut ledger = MemoryLedger::new();
        let decision = accept(&mut w, 1, &ledger).unwrap();
        ledger.preload(id[index], used_calls, used_cost, used_bytes);
        assert_eq!(
            ledger.charge(&decision),
            Err(ChargeError::LimitExhausted),
            "grant {index} calls {used_calls} cost {used_cost} bytes {used_bytes}"
        );
        assert!(!ledger.nonce_seen(&decision.invoker, &decision.nonce));
        for (other, grant_id) in id.iter().enumerate() {
            if other != index {
                assert_eq!(ledger.spent(grant_id, NOW).unwrap().calls, 0);
            }
        }
    }

    // The rate limit of the root, hit by calls recorded in the window.
    let mut w = World::new();
    let id = ids(&w.grants());
    let mut ledger = MemoryLedger::new();
    let decision = accept(&mut w, 1, &ledger).unwrap();
    for _ in 0..10 {
        ledger.record_call_at(id[0], NOW - 5);
    }
    assert_eq!(ledger.charge(&decision), Err(ChargeError::LimitExhausted));
}

#[test]
fn a_charge_up_to_the_ceiling_is_allowed_and_one_more_is_not() {
    let id = [9u8; 32];
    let mut ledger = MemoryLedger::new();
    for n in 0..3u8 {
        ledger
            .charge(&one_grant(id, calls(3), 1_000, n, 100))
            .unwrap();
    }
    assert_eq!(
        ledger.charge(&one_grant(id, calls(3), 1_000, 3, 100)),
        Err(ChargeError::LimitExhausted)
    );
    assert_eq!(ledger.spent(&id, 100).unwrap().calls, 3);
}

#[test]
fn the_rate_window_follows_the_time_passed_in() {
    let id = [9u8; 32];
    let limits = Limits {
        rate: Some(2),
        ..Limits::default()
    };
    let mut ledger = MemoryLedger::new();
    ledger
        .charge(&one_grant(id, limits, 10_000, 1, 1_000))
        .unwrap();
    ledger
        .charge(&one_grant(id, limits, 10_000, 2, 1_001))
        .unwrap();
    // Two calls in the last minute: a third is over the rate.
    assert_eq!(
        ledger.charge(&one_grant(id, limits, 10_000, 3, 1_002)),
        Err(ChargeError::LimitExhausted)
    );
    // At 1060 the call of 1000 has left the window (it counts while
    // time + 60 is above now) and the call of 1001 has not.
    assert_eq!(
        ledger
            .spent(&id, 1_000 + RATE_WINDOW_SECS)
            .unwrap()
            .recent_calls,
        1
    );
    ledger
        .charge(&one_grant(id, limits, 10_000, 4, 1_000 + RATE_WINDOW_SECS))
        .unwrap();
    // 1060 and 1001 are in the window again: full.
    assert_eq!(
        ledger.charge(&one_grant(id, limits, 10_000, 5, 1_000 + RATE_WINDOW_SECS)),
        Err(ChargeError::LimitExhausted)
    );
    // By 1121 everything has aged out.
    ledger
        .charge(&one_grant(id, limits, 10_000, 6, 1_121))
        .unwrap();
    assert_eq!(ledger.spent(&id, 1_121).unwrap().recent_calls, 1);
    // The lifetime counter keeps counting.
    assert_eq!(ledger.spent(&id, 1_121).unwrap().calls, 4);
}

#[test]
fn a_malformed_decision_is_refused() {
    let mut ledger = MemoryLedger::new();
    let mut d = one_grant([1u8; 32], calls(5), 1_000, 1, 100);
    d.chain_limits.clear();
    assert_eq!(ledger.charge(&d), Err(ChargeError::MalformedDecision));
    let mut d = one_grant([1u8; 32], calls(5), 1_000, 1, 100);
    d.chain_expiries.push(5);
    assert_eq!(ledger.charge(&d), Err(ChargeError::MalformedDecision));
    let mut d = one_grant([1u8; 32], calls(5), 1_000, 1, 100);
    d.chain_ids.push([1u8; 32]);
    d.chain_limits.push(calls(5));
    d.chain_expiries.push(1_000);
    assert_eq!(ledger.charge(&d), Err(ChargeError::MalformedDecision));
    assert_eq!(ledger.nonce_count(), 0);
    assert_eq!(ledger.grant_count(), 0);
}

#[test]
fn grants_without_limits_are_not_tracked() {
    let mut ledger = MemoryLedger::new();
    ledger
        .charge(&one_grant([1u8; 32], Limits::default(), 1_000, 1, 100))
        .unwrap();
    assert_eq!(ledger.grant_count(), 0);
    assert_eq!(ledger.nonce_count(), 1);
    assert_eq!(ledger.spent(&[1u8; 32], 100).unwrap().calls, 0);
}

#[test]
fn nonces_are_kept_until_their_horizon_and_the_ledger_refuses_rather_than_forget() {
    let config = LedgerConfig {
        max_nonces: 4,
        max_grants: 16,
    };
    let mut ledger = MemoryLedger::with_config(config);
    for n in 0..4u8 {
        ledger.charge(&nonce_only(n, 100, 160)).unwrap();
    }
    // Full of nonces that still count: refuse, and forget nothing.
    assert_eq!(
        ledger.charge(&nonce_only(9, 100, 160)),
        Err(ChargeError::LedgerFull)
    );
    assert_eq!(
        ledger.charge(&nonce_only(9, 160, 220)),
        Err(ChargeError::LedgerFull)
    );
    assert_eq!(ledger.nonce_count(), 4);
    for n in 0..4u8 {
        assert!(ledger.nonce_seen(&[0x33u8; 32], &[n; 16]));
    }
    // One second after the horizon the old ones go, and only they.
    ledger.charge(&nonce_only(9, 161, 221)).unwrap();
    assert_eq!(ledger.nonce_count(), 1);
    assert!(ledger.nonce_seen(&[0x33u8; 32], &[9u8; 16]));
    assert!(!ledger.nonce_seen(&[0x33u8; 32], &[0u8; 16]));
}

#[test]
fn eviction_pressure_never_lets_a_replay_inside_the_window_through() {
    let mut w = World::new();
    let config = LedgerConfig {
        max_nonces: 3,
        max_grants: 16,
    };
    let mut ledger = MemoryLedger::with_config(config);
    let decision = accept(&mut w, 1, &ledger).unwrap();
    ledger.charge(&decision).unwrap();

    // Fill the table with other nonces, then push on it.
    ledger.charge(&nonce_only(2, NOW + 1, NOW + 61)).unwrap();
    ledger.charge(&nonce_only(3, NOW + 2, NOW + 62)).unwrap();
    for n in 4..10u8 {
        assert_eq!(
            ledger.charge(&nonce_only(n, NOW + 3, NOW + 63)),
            Err(ChargeError::LedgerFull)
        );
    }

    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let replay_at = |now: u64, ledger: &MemoryLedger| {
        let mut c = ctx(&w, &roots);
        c.now = now;
        verify(
            &Chain::new(grants.clone()),
            &inv,
            &c,
            &RevocationSet::current(now),
            ledger,
        )
    };

    // Still inside the freshness window (the invocation is dated NOW): the
    // replay is a replay.
    assert_eq!(replay_at(NOW + 30, &ledger), Err(Denial::NonceReplayed));
    // Last second of the window: the nonce must still be there after a charge
    // evicts everything past its horizon.
    ledger
        .charge(&nonce_only(20, NOW + 60, NOW + 120))
        .unwrap_err();
    assert_eq!(replay_at(NOW + 60, &ledger), Err(Denial::NonceReplayed));

    // One second later the nonce is evicted, and the same bytes are refused
    // as stale at step 9, not accepted.
    ledger.charge(&nonce_only(21, NOW + 61, NOW + 121)).unwrap();
    assert!(!ledger.nonce_seen(&decision.invoker, &decision.nonce));
    assert_eq!(replay_at(NOW + 61, &ledger), Err(Denial::InvocationStale));
}

#[test]
fn the_nonce_horizon_covers_skew_and_the_configured_age() {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let mut c = ctx(&w, &roots);
    c.skew_secs = 120;
    c.max_invocation_age_secs = 90;
    let decision = verify(
        &Chain::new(grants),
        &inv,
        &c,
        &w.revocations(),
        &MemoryLedger::new(),
    )
    .unwrap();
    assert_eq!(decision.nonce_horizon, NOW + 120 + 90);
}

#[test]
fn usage_records_of_expired_grants_are_dropped_when_the_table_is_full() {
    let config = LedgerConfig {
        max_nonces: 100,
        max_grants: 2,
    };
    let mut ledger = MemoryLedger::with_config(config);
    ledger
        .charge(&one_grant([1u8; 32], calls(5), 500, 1, 100))
        .unwrap();
    ledger
        .charge(&one_grant([2u8; 32], calls(5), 600, 2, 100))
        .unwrap();
    // Full of live grants: a third is refused and the others are kept.
    assert_eq!(
        ledger.charge(&one_grant([3u8; 32], calls(5), 700, 3, 400)),
        Err(ChargeError::LedgerFull)
    );
    assert_eq!(ledger.grant_count(), 2);
    assert_eq!(ledger.spent(&[1u8; 32], 400).unwrap().calls, 1);
    // After the first expires (step 7 refuses it from 500) its record may go.
    ledger
        .charge(&one_grant([3u8; 32], calls(5), 700, 3, 500))
        .unwrap();
    assert_eq!(ledger.grant_count(), 2);
    assert_eq!(ledger.spent(&[1u8; 32], 500).unwrap().calls, 0);
    assert_eq!(ledger.spent(&[2u8; 32], 500).unwrap().calls, 1);
}

#[test]
fn the_rate_window_keeps_one_counter_per_second_not_one_per_call() {
    // A huge rate ceiling and many calls in one second: the count is right
    // and the answer does not need a record per call.
    let id = [9u8; 32];
    let limits = Limits {
        rate: Some(1_000_000),
        ..Limits::default()
    };
    let mut ledger = MemoryLedger::new();
    for n in 0..200u32 {
        let mut d = one_grant(id, limits, 10_000, 0, 1_000);
        d.nonce = [0u8; 16];
        d.nonce[..4].copy_from_slice(&n.to_be_bytes());
        ledger.charge(&d).unwrap();
    }
    assert_eq!(ledger.spent(&id, 1_000).unwrap().recent_calls, 200);
    assert_eq!(ledger.spent(&id, 1_059).unwrap().recent_calls, 200);
    assert_eq!(ledger.spent(&id, 1_060).unwrap().recent_calls, 0);
}
