//! Receipts: issuing, verifying, matching a receipt to an invocation and a
//! decision, and every way a receipt can be wrong.

mod common;

use common::*;
use sha2::{Digest, Sha256};
use spine_authority::{
    verify, Chain, Decision, DecodeError, Denial, Effect, Invocation, NoRevocationData, Receipt,
    ReceiptBody, ReceiptError, INVOCATION_ID_DOMAIN, MAX_RECEIPT_BYTES, RECEIPT_DOMAIN,
    RECEIPT_VERSION,
};

const RESULT: [u8; 32] = [0xcd; 32];

type Verdict = Result<Decision, Denial>;

struct Fixture {
    w: World,
    inv: Invocation,
    allowed: Verdict,
}

fn fixture() -> Fixture {
    let w = World::new();
    let grants = w.grants();
    let inv = w.invocation(&grants);
    let roots = w.roots();
    let c = ctx(&w, &roots);
    let allowed = verify(&Chain::new(grants), &inv, &c, &w.revocations(), &w.ledger());
    assert!(allowed.is_ok());
    Fixture { w, inv, allowed }
}

fn denied(f: &Fixture) -> Verdict {
    let grants = f.w.grants();
    let roots = f.w.roots();
    let c = ctx(&f.w, &roots);
    verify(
        &Chain::new(grants),
        &f.inv,
        &c,
        &NoRevocationData,
        &f.w.ledger(),
    )
}

#[test]
fn an_allowed_receipt_records_the_decision_and_verifies() {
    let f = fixture();
    let receipt = Receipt::issue(&f.w.recipient, &f.inv, &f.allowed, RESULT, NOW + 1).unwrap();
    let decision = f.allowed.as_ref().unwrap();
    let b = &receipt.body;
    assert!(RECEIPT_DOMAIN.ends_with(&[0u8]));
    assert!(INVOCATION_ID_DOMAIN.ends_with(&[0u8]));
    assert_eq!(b.version, RECEIPT_VERSION);
    assert_eq!(b.recipient, public(&f.w.recipient));
    assert_eq!(b.invocation, f.inv.id().unwrap());
    assert_eq!(b.leaf, f.inv.body.leaf);
    assert!(b.allowed);
    assert_eq!(b.effect, Some(Effect::Reversible));
    assert_eq!(b.step, None);
    assert_eq!(
        (b.calls, b.cost, b.bytes),
        (1, decision.cost, decision.bytes)
    );
    assert_eq!(b.result, RESULT);
    assert_eq!(b.time, NOW + 1);
    assert!(b.signing_bytes().unwrap().starts_with(RECEIPT_DOMAIN));

    assert_eq!(receipt.verify(&public(&f.w.recipient)), Ok(()));
    assert_eq!(receipt.matches(&f.inv, &f.allowed), Ok(()));

    let bytes = receipt.encode().unwrap();
    assert!(bytes.len() <= MAX_RECEIPT_BYTES);
    assert_eq!(Receipt::decode(&bytes).unwrap(), receipt);
}

#[test]
fn the_invocation_id_is_a_domain_separated_hash_of_the_signed_invocation() {
    let f = fixture();
    let mut hasher = Sha256::new();
    hasher.update(INVOCATION_ID_DOMAIN);
    hasher.update(f.inv.encode().unwrap().as_slice());
    let expected: [u8; 32] = hasher.finalize().into();
    assert_eq!(f.inv.id().unwrap(), expected);
}

#[test]
fn a_denial_receipt_carries_the_step_and_nothing_else() {
    let f = fixture();
    let verdict = denied(&f);
    assert_eq!(verdict, Err(Denial::RevocationDataStale));
    // The result passed in is ignored for a denial.
    let receipt = Receipt::issue(&f.w.recipient, &f.inv, &verdict, RESULT, NOW).unwrap();
    let b = &receipt.body;
    assert!(!b.allowed);
    assert_eq!(b.effect, None);
    assert_eq!(b.step, Some(8));
    assert_eq!((b.calls, b.cost, b.bytes), (0, 0, 0));
    assert_eq!(b.result, [0u8; 32]);
    assert_eq!(receipt.verify(&public(&f.w.recipient)), Ok(()));
    assert_eq!(receipt.matches(&f.inv, &verdict), Ok(()));

    // Two different denials of the same step give the same receipt body.
    let other = Receipt::issue(&f.w.recipient, &f.inv, &Err(Denial::Revoked), RESULT, NOW).unwrap();
    assert_eq!(other.body, receipt.body);
    assert_eq!(other.signature, receipt.signature);

    // A different step gives a different receipt.
    let step9 = Receipt::issue(
        &f.w.recipient,
        &f.inv,
        &Err(Denial::NonceReplayed),
        RESULT,
        NOW,
    )
    .unwrap();
    assert_eq!(step9.body.step, Some(9));
    assert_ne!(step9.body, receipt.body);
}

#[test]
fn a_receipt_can_only_be_issued_by_the_recipient() {
    let f = fixture();
    assert_eq!(
        Receipt::issue(&f.w.keys[0], &f.inv, &f.allowed, RESULT, NOW),
        Err(ReceiptError::NotRecipient)
    );
}

#[test]
fn a_receipt_is_refused_for_the_wrong_expected_recipient() {
    let f = fixture();
    let receipt = Receipt::issue(&f.w.recipient, &f.inv, &f.allowed, RESULT, NOW).unwrap();
    assert_eq!(
        receipt.verify(&public(&f.w.keys[0])),
        Err(ReceiptError::WrongRecipient)
    );
}

#[test]
fn a_receipt_signed_by_someone_else_is_refused() {
    let f = fixture();
    let receipt = Receipt::issue(&f.w.recipient, &f.inv, &f.allowed, RESULT, NOW).unwrap();
    let forged = Receipt::sign_unchecked(&f.w.keys[0], receipt.body.clone()).unwrap();
    assert_eq!(
        forged.verify(&public(&f.w.recipient)),
        Err(ReceiptError::BadSignature)
    );
}

#[test]
fn a_bad_signature_is_refused() {
    let f = fixture();
    let mut receipt = Receipt::issue(&f.w.recipient, &f.inv, &f.allowed, RESULT, NOW).unwrap();
    receipt.signature[0] ^= 1;
    assert_eq!(
        receipt.verify(&public(&f.w.recipient)),
        Err(ReceiptError::BadSignature)
    );
}

#[test]
fn tampering_with_any_field_is_refused() {
    let f = fixture();
    let receipt = Receipt::issue(&f.w.recipient, &f.inv, &f.allowed, RESULT, NOW).unwrap();
    let who = public(&f.w.recipient);

    // Fields that keep the receipt well formed break the signature.
    let tampers: [fn(&mut ReceiptBody); 8] = [
        |b| b.invocation[0] ^= 1,
        |b| b.leaf[0] ^= 1,
        |b| b.effect = Some(Effect::Read),
        |b| b.cost += 1,
        |b| b.bytes += 1,
        |b| b.result[0] ^= 1,
        |b| b.time += 1,
        |b| b.recipient[0] ^= 1,
    ];
    for (index, tamper) in tampers.iter().enumerate() {
        let mut t = receipt.clone();
        tamper(&mut t.body);
        let expected = if index == 7 {
            ReceiptError::WrongRecipient
        } else {
            ReceiptError::BadSignature
        };
        assert_eq!(t.verify(&who), Err(expected), "tamper {index}");
    }

    // Fields that make it inconsistent are malformed, even if re-signed.
    let inconsistent: [fn(&mut ReceiptBody); 6] = [
        |b| b.allowed = false,
        |b| b.step = Some(3),
        |b| b.effect = None,
        |b| b.calls = 2,
        |b| b.version = 2,
        |b| b.calls = 0,
    ];
    for (index, change) in inconsistent.iter().enumerate() {
        let mut body = receipt.body.clone();
        change(&mut body);
        let signed = Receipt::sign_unchecked(&f.w.recipient, body).unwrap();
        assert_eq!(
            signed.verify(&who),
            Err(ReceiptError::Malformed),
            "inconsistent {index}"
        );
    }
}

#[test]
fn a_denial_receipt_that_says_more_than_the_step_is_malformed() {
    let f = fixture();
    let verdict = denied(&f);
    let receipt = Receipt::issue(&f.w.recipient, &f.inv, &verdict, RESULT, NOW).unwrap();
    let who = public(&f.w.recipient);
    let extras: [fn(&mut ReceiptBody); 7] = [
        |b| b.cost = 1,
        |b| b.bytes = 1,
        |b| b.calls = 1,
        |b| b.result = [1u8; 32],
        |b| b.effect = Some(Effect::Read),
        |b| b.step = Some(12),
        |b| b.step = Some(0),
    ];
    for (index, extra) in extras.iter().enumerate() {
        let mut body = receipt.body.clone();
        extra(&mut body);
        let signed = Receipt::sign_unchecked(&f.w.recipient, body).unwrap();
        assert_eq!(
            signed.verify(&who),
            Err(ReceiptError::Malformed),
            "extra {index}"
        );
    }
    let mut no_step = receipt.body.clone();
    no_step.step = None;
    let signed = Receipt::sign_unchecked(&f.w.recipient, no_step).unwrap();
    assert_eq!(signed.verify(&who), Err(ReceiptError::Malformed));
}

#[test]
fn a_receipt_for_a_different_invocation_does_not_match() {
    let f = fixture();
    let receipt = Receipt::issue(&f.w.recipient, &f.inv, &f.allowed, RESULT, NOW).unwrap();

    let mut other = World::new();
    other.inv.nonce = [8u8; 16];
    let grants = other.grants();
    let other_inv = other.invocation(&grants);
    assert_eq!(
        receipt.matches(&other_inv, &f.allowed),
        Err(ReceiptError::InvocationMismatch)
    );
}

#[test]
fn a_receipt_naming_another_last_grant_does_not_match() {
    let f = fixture();
    let receipt = Receipt::issue(&f.w.recipient, &f.inv, &f.allowed, RESULT, NOW).unwrap();
    let mut body = receipt.body.clone();
    body.leaf[0] ^= 1;
    let other = Receipt::sign_unchecked(&f.w.recipient, body).unwrap();
    assert_eq!(
        other.matches(&f.inv, &f.allowed),
        Err(ReceiptError::LeafMismatch)
    );
}

#[test]
fn a_receipt_does_not_match_a_different_decision() {
    let f = fixture();
    let receipt = Receipt::issue(&f.w.recipient, &f.inv, &f.allowed, RESULT, NOW).unwrap();

    // Recorded as allowed, but the verdict was a denial.
    assert_eq!(
        receipt.matches(&f.inv, &denied(&f)),
        Err(ReceiptError::OutcomeMismatch)
    );

    // Other amounts or another effect class.
    let mut decision = f.allowed.clone().unwrap();
    decision.cost += 1;
    assert_eq!(
        receipt.matches(&f.inv, &Ok(decision)),
        Err(ReceiptError::OutcomeMismatch)
    );
    let mut decision = f.allowed.clone().unwrap();
    decision.bytes += 1;
    assert_eq!(
        receipt.matches(&f.inv, &Ok(decision)),
        Err(ReceiptError::OutcomeMismatch)
    );
    let mut decision = f.allowed.clone().unwrap();
    decision.effect = Effect::Read;
    assert_eq!(
        receipt.matches(&f.inv, &Ok(decision)),
        Err(ReceiptError::OutcomeMismatch)
    );

    // A denial receipt against another step.
    let verdict = denied(&f);
    let denial = Receipt::issue(&f.w.recipient, &f.inv, &verdict, RESULT, NOW).unwrap();
    assert_eq!(
        denial.matches(&f.inv, &Err(Denial::NonceReplayed)),
        Err(ReceiptError::OutcomeMismatch)
    );
    assert_eq!(
        denial.matches(&f.inv, &f.allowed),
        Err(ReceiptError::OutcomeMismatch)
    );
}

#[test]
fn a_decision_for_another_invocation_cannot_be_receipted() {
    let f = fixture();
    let mut decision = f.allowed.clone().unwrap();
    decision.nonce = [0u8; 16];
    assert_eq!(
        Receipt::issue(&f.w.recipient, &f.inv, &Ok(decision), RESULT, NOW),
        Err(ReceiptError::DecisionMismatch)
    );
    let mut decision = f.allowed.clone().unwrap();
    decision.chain_ids.pop();
    assert_eq!(
        Receipt::issue(&f.w.recipient, &f.inv, &Ok(decision), RESULT, NOW),
        Err(ReceiptError::DecisionMismatch)
    );
}

#[test]
fn a_receipt_whose_result_was_changed_after_signing_does_not_verify() {
    let f = fixture();
    let receipt = Receipt::issue(&f.w.recipient, &f.inv, &f.allowed, RESULT, NOW).unwrap();
    // matches() does not look at the result digest, verify() covers it.
    let mut t = receipt.clone();
    t.body.result = [0u8; 32];
    assert_eq!(t.matches(&f.inv, &f.allowed), Ok(()));
    assert_eq!(
        t.verify(&public(&f.w.recipient)),
        Err(ReceiptError::BadSignature)
    );
}

#[test]
fn receipt_decoding_refuses_what_is_not_the_canonical_encoding() {
    let f = fixture();
    let receipt = Receipt::issue(&f.w.recipient, &f.inv, &f.allowed, RESULT, NOW).unwrap();
    let bytes = receipt.encode().unwrap();

    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(Receipt::decode(&trailing).is_err());
    assert!(Receipt::decode(&bytes[..bytes.len() - 2]).is_err());
    assert!(Receipt::decode(&with_extra_field(&bytes)).is_err());
    assert!(Receipt::decode(&with_long_form_version(&bytes)).is_err());
    assert_eq!(
        Receipt::decode(&[0u8; MAX_RECEIPT_BYTES + 1]),
        Err(DecodeError::TooLarge(MAX_RECEIPT_BYTES))
    );
}

#[test]
fn the_largest_receipt_fits_the_bound() {
    let f = fixture();
    let receipt = Receipt::issue(&f.w.recipient, &f.inv, &f.allowed, RESULT, NOW).unwrap();
    let mut body = receipt.body.clone();
    body.effect = Some(Effect::Irreversible);
    body.cost = u64::MAX;
    body.bytes = u64::MAX;
    body.time = u64::MAX;
    let big = Receipt::sign_unchecked(&f.w.recipient, body).unwrap();
    let bytes = big.encode().unwrap();
    assert!(bytes.len() <= MAX_RECEIPT_BYTES, "{} bytes", bytes.len());
    assert_eq!(Receipt::decode(&bytes).unwrap(), big);
}
