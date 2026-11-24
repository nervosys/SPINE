//! Verifying what a node says about a name (`docs/spec/00-wire.md` 12.5,
//! `docs/spec/02-names.md` 10.10 decision 53).
//!
//! A node's answer to a resolution is evidence, never a verdict. This module is
//! the client's half of that rule:
//!
//! * `resolved`: the log is replayed from the name's inception key
//!   ([`spine_name::SuccessionLog::verify`]) and the record is judged against
//!   the state it yields ([`crate::resources::authorise_with_log`]). A record
//!   that needs a log the node did not send is [`spine_name::NameError::NeedSuccessionLog`].
//! * `retired`: the log verifies and ends in a `retire` entry.
//! * `compromised`: both logs verify from the inception key and neither is a
//!   prefix of the other ([`spine_name::detect_equivocation`]): the proof.
//! * `revoked`: the record is a tombstone, it has not lapsed, and it is signed by
//!   the key the verified log makes current.
//!
//! Evidence that does not verify is [`NameOutcomeError::InvalidEvidence`]: the
//! node sent something wrong or hostile, which is not the same as the name being
//! absent, and callers must not treat it as not found.

use std::collections::HashSet;
use std::fmt;

use serde_json::Value;
use spine_name::{
    detect_equivocation, log_within_bounds, Equivocation, LogBook, LogOffer, NameError, NameRecord,
    SpineUri, SuccessionError, SuccessionLog, SuccessionState,
};
use spine_protocol::{NameProvenance, NameResolution};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::resources::{authorise, authorise_with_log};
use crate::AgentClient;

/// A resolution whose evidence this client has checked.
#[derive(Debug, Clone, PartialEq)]
pub enum VerifiedResolution {
    /// The record is authorised by `log`, which verified from the name's
    /// inception key (empty for a name that never rotated).
    Resolved {
        record: NameRecord,
        provenance: NameProvenance,
        log: SuccessionLog,
    },
    /// The validator the caller sent still matches.
    Unchanged {
        ttl_secs: u32,
    },
    NotFound {
        name: String,
    },
    Invalid {
        name: String,
        reason: String,
    },
    Unavailable {
        name: String,
        reason: String,
    },
    /// `log` verifies and its last entry retires the name for good.
    Retired {
        name: String,
        log: SuccessionLog,
    },
    /// The two logs are an equivocation proof for the name.
    Compromised {
        name: String,
        proof: Box<Equivocation>,
    },
    /// `record` is a tombstone, signed by the key `log` makes current. A
    /// tombstone lapses: this is not permanent.
    Revoked {
        record: NameRecord,
        provenance: NameProvenance,
        log: SuccessionLog,
    },
}

/// A resolution that did not give a usable record, as an error: the typed form
/// the resource paths return, so a caller can tell a verified retirement from a
/// name nobody published and from a node that lied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameOutcomeError {
    /// The node showed a succession log that retires the name, and it verified.
    Retired { name: String },
    /// The node showed two conflicting logs of the name, and they verified.
    Compromised { name: String },
    /// The node showed a tombstone for the name, and it verified.
    Revoked { name: String },
    /// The node's evidence does not verify: it is wrong or hostile. Never a
    /// reason to believe the name is absent.
    InvalidEvidence {
        outcome: &'static str,
        name: String,
        reason: String,
    },
}

impl fmt::Display for NameOutcomeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameOutcomeError::Retired { name } => {
                write!(
                    f,
                    "name {name} is retired: its succession log ends in a retirement"
                )
            }
            NameOutcomeError::Compromised { name } => write!(
                f,
                "name {name} is compromised: two conflicting succession logs prove equivocation"
            ),
            NameOutcomeError::Revoked { name } => {
                write!(f, "name {name} is revoked by a tombstone")
            }
            NameOutcomeError::InvalidEvidence {
                outcome,
                name,
                reason,
            } => write!(
                f,
                "the node sent invalid evidence for a {outcome} answer about {name}: {reason}"
            ),
        }
    }
}

impl std::error::Error for NameOutcomeError {}

fn invalid(outcome: &'static str, name: &str, reason: impl fmt::Display) -> anyhow::Error {
    anyhow::Error::new(NameOutcomeError::InvalidEvidence {
        outcome,
        name: name.to_string(),
        reason: reason.to_string(),
    })
}

/// The inception key a name certifies against: `None` for a name with no key
/// (`blob:`, `host:`), which has no succession log.
fn name_key(name: &str, outcome: &'static str) -> anyhow::Result<Option<[u8; 32]>> {
    let uri = SpineUri::parse(name).map_err(|e| invalid(outcome, name, e))?;
    Ok(uri.public_key().copied())
}

/// A log read from the wire's JSON entries and replayed from `key`. Over the
/// size bounds, not a log, or an entry that does not verify is an error.
fn verified_log(
    key: &[u8; 32],
    entries: &[Value],
) -> Result<(SuccessionLog, SuccessionState), String> {
    let log: SuccessionLog = serde_json::from_value(Value::Array(entries.to_vec()))
        .map_err(|e| format!("not a succession log: {e}"))?;
    if !log_within_bounds(&log) {
        return Err("a succession log over the size bounds".to_string());
    }
    let state = log
        .verify(key)
        .map_err(|e: SuccessionError| format!("succession log does not verify: {e}"))?;
    Ok((log, state))
}

/// The record of a `resolved` or `revoked` answer.
fn read_record(outcome: &'static str, name: &str, value: Value) -> anyhow::Result<NameRecord> {
    serde_json::from_value(value)
        .map_err(|e| invalid(outcome, name, format!("not a name record: {e}")))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Verify a resolution `name` got, with this machine's clock. See
/// [`verify_resolution_at`].
pub fn verify_resolution(
    name: &str,
    resolution: NameResolution,
) -> anyhow::Result<VerifiedResolution> {
    verify_resolution_at(name, resolution, now())
}

/// Verify a node's answer about `name` at time `now` (Unix seconds, used only
/// for a tombstone's lapse).
///
/// An answer without evidence (`unchanged`, `not_found`, `invalid`,
/// `unavailable`) passes through. Every answer that carries evidence is checked
/// as the module documentation says: if the evidence does not verify the error
/// is [`NameOutcomeError::InvalidEvidence`], and if a `resolved` record is not
/// authorised by its own log the error is the [`NameError`] that says why
/// (`NeedSuccessionLog` when the node sent too short a log, `SupersededKey`,
/// `Retired`, `BadSignature`).
pub fn verify_resolution_at(
    name: &str,
    resolution: NameResolution,
    now: u64,
) -> anyhow::Result<VerifiedResolution> {
    match resolution {
        NameResolution::Unchanged { ttl_secs } => Ok(VerifiedResolution::Unchanged { ttl_secs }),
        NameResolution::NotFound { name } => Ok(VerifiedResolution::NotFound { name }),
        NameResolution::Invalid { name, reason } => {
            Ok(VerifiedResolution::Invalid { name, reason })
        }
        NameResolution::Unavailable { name, reason } => {
            Ok(VerifiedResolution::Unavailable { name, reason })
        }
        NameResolution::Resolved {
            record,
            provenance,
            log,
        } => {
            let outcome = "resolved";
            let record = read_record(outcome, name, record)?;
            let key = name_key(name, outcome)?;
            if record.name.public_key() != key.as_ref() {
                return Err(invalid(outcome, name, "the record is for another name"));
            }
            let log = match key {
                Some(key) => {
                    verified_log(&key, &log)
                        .map_err(|reason| invalid(outcome, name, reason))?
                        .0
                }
                None if log.is_empty() => SuccessionLog::default(),
                None => return Err(invalid(outcome, name, "a name with no key has no log")),
            };
            judge(&record, &log)?;
            Ok(VerifiedResolution::Resolved {
                record,
                provenance,
                log,
            })
        }
        NameResolution::Retired { name: said, log } => {
            let outcome = "retired";
            let key = name_key(name, outcome)?
                .ok_or_else(|| invalid(outcome, name, "a name with no key has no log"))?;
            let (log, state) =
                verified_log(&key, &log).map_err(|reason| invalid(outcome, name, reason))?;
            if !state.is_retired() {
                return Err(invalid(outcome, name, "the log does not retire the name"));
            }
            Ok(VerifiedResolution::Retired { name: said, log })
        }
        NameResolution::Compromised { name: said, logs } => {
            let outcome = "compromised";
            let key = name_key(name, outcome)?
                .ok_or_else(|| invalid(outcome, name, "a name with no key has no log"))?;
            let [first, second] = <[Vec<Value>; 2]>::try_from(logs)
                .map_err(|_| invalid(outcome, name, "not exactly two logs"))?;
            let first = verified_log(&key, &first)
                .map_err(|reason| invalid(outcome, name, format!("first log: {reason}")))?
                .0;
            let second = verified_log(&key, &second)
                .map_err(|reason| invalid(outcome, name, format!("second log: {reason}")))?
                .0;
            match detect_equivocation(&key, &first, &second) {
                Ok(Some(proof)) => Ok(VerifiedResolution::Compromised {
                    name: said,
                    proof: Box::new(proof),
                }),
                Ok(None) => Err(invalid(
                    outcome,
                    name,
                    "one log is a prefix of the other: they do not conflict",
                )),
                Err(e) => Err(invalid(outcome, name, e)),
            }
        }
        NameResolution::Revoked {
            record,
            provenance,
            log,
        } => {
            let outcome = "revoked";
            let record = read_record(outcome, name, record)?;
            let key = name_key(name, outcome)?
                .ok_or_else(|| invalid(outcome, name, "a name with no key has no tombstone"))?;
            if record.name.public_key() != Some(&key) {
                return Err(invalid(outcome, name, "the tombstone is for another name"));
            }
            if !record.is_tombstone() {
                return Err(invalid(outcome, name, "the record is not a tombstone"));
            }
            if record.is_expired(now) {
                return Err(invalid(outcome, name, "the tombstone has lapsed"));
            }
            let (log, _) =
                verified_log(&key, &log).map_err(|reason| invalid(outcome, name, reason))?;
            // A tombstone is a record: the key the log makes current signs it.
            authorise_with_log(&record, &log)
                .map_err(|e| invalid(outcome, name, format!("the tombstone: {e}")))?;
            Ok(VerifiedResolution::Revoked {
                record,
                provenance,
                log,
            })
        }
    }
}

/// Judge `record` against `log`: an empty log is the inception key's, so a
/// record with no head claim is checked as before and one with a claim needs
/// its log.
fn judge(record: &NameRecord, log: &SuccessionLog) -> anyhow::Result<()> {
    authorise(record, Some(log)).map_err(anyhow::Error::new)
}

/// The log a client holds from two sources, by the rule a verifier's book
/// keeps ([`LogBook::plan`]): each verifies from `key`, a log that is a prefix
/// of the other gives way to the longer, and two that conflict are an
/// equivocation proof, which is [`NameOutcomeError::Compromised`].
pub fn combine_logs(
    name: &str,
    key: &[u8; 32],
    from_node: &SuccessionLog,
    from_caller: &SuccessionLog,
) -> anyhow::Result<SuccessionLog> {
    let mut book = LogBook::new();
    for log in [from_node, from_caller] {
        let offer = book
            .plan(key, log.clone(), &HashSet::new())
            .map_err(|e| anyhow::Error::new(NameError::InvalidSuccessionLog(e.to_string())))?;
        if matches!(offer, LogOffer::Conflict(_)) {
            return Err(anyhow::Error::new(NameOutcomeError::Compromised {
                name: name.to_string(),
            }));
        }
        book.apply(offer);
    }
    Ok(book.log(key).cloned().unwrap_or_default())
}

impl VerifiedResolution {
    /// The wire name of the outcome (`docs/spec/00-wire.md` 12.5): `resolved`,
    /// `unchanged`, `not_found`, `invalid`, `unavailable`, `retired`,
    /// `compromised` or `revoked`.
    pub fn outcome(&self) -> &'static str {
        match self {
            VerifiedResolution::Resolved { .. } => "resolved",
            VerifiedResolution::Unchanged { .. } => "unchanged",
            VerifiedResolution::NotFound { .. } => "not_found",
            VerifiedResolution::Invalid { .. } => "invalid",
            VerifiedResolution::Unavailable { .. } => "unavailable",
            VerifiedResolution::Retired { .. } => "retired",
            VerifiedResolution::Compromised { .. } => "compromised",
            VerifiedResolution::Revoked { .. } => "revoked",
        }
    }

    /// The record and log of a `resolved` answer; every other outcome is an
    /// error: a verified retirement, compromise or revocation is the matching
    /// [`NameOutcomeError`], and an answer with no record is a plain failure.
    pub fn into_record(self, name: &str) -> anyhow::Result<(NameRecord, SuccessionLog)> {
        match self {
            VerifiedResolution::Resolved { record, log, .. } => Ok((record, log)),
            VerifiedResolution::Retired { .. } => {
                Err(anyhow::Error::new(NameOutcomeError::Retired {
                    name: name.to_string(),
                }))
            }
            VerifiedResolution::Compromised { .. } => {
                Err(anyhow::Error::new(NameOutcomeError::Compromised {
                    name: name.to_string(),
                }))
            }
            VerifiedResolution::Revoked { .. } => {
                Err(anyhow::Error::new(NameOutcomeError::Revoked {
                    name: name.to_string(),
                }))
            }
            other => anyhow::bail!("resource did not resolve: {other:?}"),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> AgentClient<S> {
    /// [`AgentClient::resolve_name`], with the evidence checked
    /// ([`verify_resolution`]): the log replayed from the name's inception key
    /// and the record judged against it, a retirement, a compromise or a
    /// revocation shown by evidence that verifies. The node is not trusted: a
    /// wrong or truncated log can only make this an error.
    pub async fn resolve_name_verified(
        &mut self,
        name: &str,
        if_none_match: Option<[u8; 32]>,
    ) -> anyhow::Result<VerifiedResolution> {
        let answer = self.resolve_name(name, if_none_match).await?;
        verify_resolution(name, answer)
    }

    /// [`AgentClient::resolve_names`], each answer verified on its own: one
    /// node answer that does not verify is that item's error, not the batch's.
    pub async fn resolve_names_verified(
        &mut self,
        names: &[String],
    ) -> anyhow::Result<Vec<anyhow::Result<VerifiedResolution>>> {
        let answers = self.resolve_names(names).await?;
        anyhow::ensure!(
            answers.len() == names.len(),
            "the node answered {} names for {} asked",
            answers.len(),
            names.len()
        );
        Ok(names
            .iter()
            .zip(answers)
            .map(|(name, answer)| verify_resolution(name, answer))
            .collect())
    }

    /// [`AgentClient::fetch_name`], with the resolution verified: the endpoints
    /// to dial, in priority order, and the checked answer.
    pub async fn fetch_name_verified(
        &mut self,
        name: &str,
        if_none_match: Option<[u8; 32]>,
    ) -> anyhow::Result<(VerifiedResolution, Vec<String>)> {
        let body = self.fetch_name(name, if_none_match).await?;
        let resolution: NameResolution = serde_json::from_value(body["resolution"].clone())?;
        let endpoints: Vec<String> = serde_json::from_value(body["endpoints"].clone())?;
        Ok((verify_resolution(name, resolution)?, endpoints))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spine_name::{Endpoint, SUCCESSION_META_KEY};
    use spine_protocol::identify::SigningKey;

    const NOW: u64 = 1_700_000_000;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn public(key: &SigningKey) -> [u8; 32] {
        key.verifying_key().to_bytes()
    }

    fn entries(log: &SuccessionLog) -> Vec<Value> {
        match serde_json::to_value(log).unwrap() {
            Value::Array(entries) => entries,
            other => panic!("{other}"),
        }
    }

    /// A name rotated once (`seed` inception, `seed + 1` current, `seed + 2`
    /// next) and its helpers.
    struct Rotated {
        name: String,
        inception: SigningKey,
        current: SigningKey,
        /// The key the rotation committed to: the next to reveal.
        next: SigningKey,
        log: SuccessionLog,
    }

    impl Rotated {
        fn new(seed: u8) -> Self {
            let inception = key(seed);
            let current = key(seed + 1);
            let next = key(seed + 2);
            let log = SuccessionLog::default()
                .rotated(
                    &public(&inception),
                    &inception,
                    &public(&current),
                    &public(&next),
                    NOW - 60,
                )
                .unwrap();
            Self {
                name: SpineUri::did(public(&inception)).to_string(),
                inception,
                current,
                next,
                log,
            }
        }

        fn state(&self) -> SuccessionState {
            self.log.verify(&public(&self.inception)).unwrap()
        }

        fn uri(&self) -> SpineUri {
            SpineUri::did(public(&self.inception))
        }

        fn record(&self, seq: u64) -> NameRecord {
            let mut record = NameRecord::new(self.uri(), seq, NOW)
                .unwrap()
                .with_endpoint(Endpoint::new("tcp", "127.0.0.1:9440"));
            record.sign_current(&self.current, &self.state()).unwrap();
            record
        }

        fn resolved(&self, record: &NameRecord, log: &SuccessionLog) -> NameResolution {
            NameResolution::Resolved {
                record: serde_json::to_value(record).unwrap(),
                provenance: NameProvenance::Network,
                log: entries(log),
            }
        }
    }

    fn outcome_error(error: &anyhow::Error) -> &NameOutcomeError {
        error
            .downcast_ref::<NameOutcomeError>()
            .unwrap_or_else(|| panic!("not an outcome error: {error:#}"))
    }

    #[test]
    fn a_rotated_name_resolved_with_its_log_verifies() {
        let rotated = Rotated::new(10);
        let record = rotated.record(2);
        let verified =
            verify_resolution_at(&rotated.name, rotated.resolved(&record, &rotated.log), NOW)
                .unwrap();
        assert_eq!(
            verified,
            VerifiedResolution::Resolved {
                record,
                provenance: NameProvenance::Network,
                log: rotated.log.clone(),
            }
        );
    }

    #[test]
    fn a_rotated_record_without_the_log_it_needs_is_an_error() {
        let rotated = Rotated::new(20);
        let record = rotated.record(2);
        let error = verify_resolution_at(
            &rotated.name,
            rotated.resolved(&record, &SuccessionLog::default()),
            NOW,
        )
        .unwrap_err();
        assert!(
            matches!(
                error.downcast_ref::<NameError>(),
                Some(NameError::NeedSuccessionLog { .. })
            ),
            "{error:#}"
        );
    }

    #[test]
    fn a_name_that_never_rotated_resolves_with_an_empty_log() {
        let inception = key(30);
        let uri = SpineUri::did(public(&inception));
        let mut record = NameRecord::new(uri.clone(), 1, NOW).unwrap();
        record.sign(&inception).unwrap();
        let answer = NameResolution::Resolved {
            record: serde_json::to_value(&record).unwrap(),
            provenance: NameProvenance::Local,
            log: Vec::new(),
        };
        let verified = verify_resolution_at(&uri.to_string(), answer, NOW).unwrap();
        assert!(matches!(
            verified,
            VerifiedResolution::Resolved { ref log, .. } if log.entries.is_empty()
        ));
    }

    #[test]
    fn a_log_that_does_not_verify_is_invalid_evidence_not_absence() {
        let rotated = Rotated::new(40);
        let record = rotated.record(2);
        // One byte of the signature changed.
        let mut entries = entries(&rotated.log);
        let signature = entries[0]["signature"].as_str().unwrap().to_string();
        let flipped = if signature.starts_with('a') { 'b' } else { 'a' };
        entries[0]["signature"] = Value::String(format!("{flipped}{}", &signature[1..]));
        let answer = NameResolution::Resolved {
            record: serde_json::to_value(&record).unwrap(),
            provenance: NameProvenance::Network,
            log: entries,
        };
        let error = verify_resolution_at(&rotated.name, answer, NOW).unwrap_err();
        assert!(matches!(
            outcome_error(&error),
            NameOutcomeError::InvalidEvidence {
                outcome: "resolved",
                ..
            }
        ));
    }

    #[test]
    fn a_record_of_another_name_is_invalid_evidence() {
        let rotated = Rotated::new(50);
        let other = Rotated::new(60);
        let record = other.record(2);
        let error = verify_resolution_at(&rotated.name, other.resolved(&record, &other.log), NOW)
            .unwrap_err();
        assert!(matches!(
            outcome_error(&error),
            NameOutcomeError::InvalidEvidence { .. }
        ));
    }

    #[test]
    fn a_superseded_keys_record_is_refused_even_with_the_log() {
        let rotated = Rotated::new(70);
        let mut stale = NameRecord::new(rotated.uri(), 3, NOW).unwrap();
        stale.sign(&rotated.inception).unwrap();
        let error =
            verify_resolution_at(&rotated.name, rotated.resolved(&stale, &rotated.log), NOW)
                .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<NameError>(),
            Some(NameError::SupersededKey(_))
        ));
    }

    fn retired(rotated: &Rotated) -> SuccessionLog {
        rotated
            .log
            .retired(
                &public(&rotated.inception),
                &rotated.current,
                &public(&rotated.next),
                NOW,
            )
            .unwrap()
    }

    #[test]
    fn a_retirement_is_shown_by_a_log_that_ends_in_retire() {
        let rotated = Rotated::new(80);
        let log = retired(&rotated);
        let answer = NameResolution::Retired {
            name: rotated.name.clone(),
            log: entries(&log),
        };
        let verified = verify_resolution_at(&rotated.name, answer, NOW).unwrap();
        assert_eq!(
            verified,
            VerifiedResolution::Retired {
                name: rotated.name.clone(),
                log,
            }
        );
        let error = verified.into_record(&rotated.name).unwrap_err();
        assert_eq!(
            outcome_error(&error),
            &NameOutcomeError::Retired {
                name: rotated.name.clone()
            }
        );
    }

    #[test]
    fn a_log_that_does_not_end_in_retire_is_not_a_retirement() {
        let rotated = Rotated::new(90);
        let answer = NameResolution::Retired {
            name: rotated.name.clone(),
            log: entries(&rotated.log),
        };
        let error = verify_resolution_at(&rotated.name, answer, NOW).unwrap_err();
        assert!(matches!(
            outcome_error(&error),
            NameOutcomeError::InvalidEvidence {
                outcome: "retired",
                ..
            }
        ));
    }

    fn two_branches(seed: u8) -> (String, SuccessionLog, SuccessionLog) {
        let inception = key(seed);
        let branch = |reveal: u8| {
            SuccessionLog::default()
                .rotated(
                    &public(&inception),
                    &inception,
                    &public(&key(reveal)),
                    &public(&key(reveal + 1)),
                    NOW,
                )
                .unwrap()
        };
        (
            SpineUri::did(public(&inception)).to_string(),
            branch(seed + 1),
            branch(seed + 3),
        )
    }

    #[test]
    fn two_conflicting_logs_are_a_compromise() {
        let (name, a, b) = two_branches(100);
        let answer = NameResolution::Compromised {
            name: name.clone(),
            logs: vec![entries(&a), entries(&b)],
        };
        let verified = verify_resolution_at(&name, answer, NOW).unwrap();
        let VerifiedResolution::Compromised { proof, .. } = &verified else {
            panic!("{verified:?}")
        };
        proof.verify(&public(&key(100))).unwrap();
        let error = verified.into_record(&name).unwrap_err();
        assert_eq!(
            outcome_error(&error),
            &NameOutcomeError::Compromised { name }
        );
    }

    #[test]
    fn logs_that_do_not_conflict_are_not_a_compromise() {
        let (name, a, _) = two_branches(110);
        // The same log twice, and a log with its own extension.
        let longer = a
            .rotated(
                &public(&key(110)),
                &key(111),
                &public(&key(112)),
                &public(&key(113)),
                NOW,
            )
            .unwrap();
        for logs in [
            vec![entries(&a), entries(&a)],
            vec![entries(&a), entries(&longer)],
        ] {
            let answer = NameResolution::Compromised {
                name: name.clone(),
                logs,
            };
            let error = verify_resolution_at(&name, answer, NOW).unwrap_err();
            assert!(matches!(
                outcome_error(&error),
                NameOutcomeError::InvalidEvidence {
                    outcome: "compromised",
                    ..
                }
            ));
        }
    }

    #[test]
    fn a_compromise_with_a_log_that_does_not_verify_is_invalid() {
        let (name, a, b) = two_branches(120);
        let mut forged = entries(&b);
        forged[0]["effective_at"] = serde_json::json!(NOW + 1);
        let answer = NameResolution::Compromised {
            name: name.clone(),
            logs: vec![entries(&a), forged],
        };
        let error = verify_resolution_at(&name, answer, NOW).unwrap_err();
        assert!(matches!(
            outcome_error(&error),
            NameOutcomeError::InvalidEvidence { .. }
        ));
    }

    fn tombstone(signer: &SigningKey, uri: &SpineUri, seq: u64, at: u64) -> NameRecord {
        let mut record = NameRecord::tombstone(uri.clone(), seq, at).unwrap();
        record.sign(signer).unwrap();
        record
    }

    #[test]
    fn a_tombstone_signed_by_the_names_key_is_a_revocation() {
        let inception = key(130);
        let uri = SpineUri::did(public(&inception));
        let record = tombstone(&inception, &uri, 2, NOW);
        let answer = NameResolution::Revoked {
            record: serde_json::to_value(&record).unwrap(),
            provenance: NameProvenance::Local,
            log: Vec::new(),
        };
        let verified = verify_resolution_at(&uri.to_string(), answer, NOW).unwrap();
        assert!(matches!(verified, VerifiedResolution::Revoked { .. }));
        let error = verified.into_record(&uri.to_string()).unwrap_err();
        assert!(matches!(
            outcome_error(&error),
            NameOutcomeError::Revoked { .. }
        ));
    }

    #[test]
    fn a_tombstone_of_a_rotated_name_is_judged_under_the_current_key() {
        let rotated = Rotated::new(140);
        let mut record = NameRecord::tombstone(rotated.uri(), 3, NOW).unwrap();
        record
            .sign_current(&rotated.current, &rotated.state())
            .unwrap();
        assert!(!record.meta.contains_key(SUCCESSION_META_KEY));
        let answer = |log: &SuccessionLog| NameResolution::Revoked {
            record: serde_json::to_value(&record).unwrap(),
            provenance: NameProvenance::Network,
            log: entries(log),
        };
        verify_resolution_at(&rotated.name, answer(&rotated.log), NOW).unwrap();
        // Without the log the key is the inception key, which did not sign it.
        let error = verify_resolution_at(&rotated.name, answer(&SuccessionLog::default()), NOW)
            .unwrap_err();
        assert!(matches!(
            outcome_error(&error),
            NameOutcomeError::InvalidEvidence {
                outcome: "revoked",
                ..
            }
        ));
    }

    #[test]
    fn a_tombstone_that_is_not_one_or_has_lapsed_is_invalid() {
        let inception = key(150);
        let uri = SpineUri::did(public(&inception));
        let mut live = NameRecord::new(uri.clone(), 2, NOW).unwrap();
        live.sign(&inception).unwrap();
        let answer = |record: &NameRecord| NameResolution::Revoked {
            record: serde_json::to_value(record).unwrap(),
            provenance: NameProvenance::Local,
            log: Vec::new(),
        };
        let error = verify_resolution_at(&uri.to_string(), answer(&live), NOW).unwrap_err();
        assert!(matches!(
            outcome_error(&error),
            NameOutcomeError::InvalidEvidence { .. }
        ));
        let record = tombstone(&inception, &uri, 2, NOW);
        let later = NOW + u64::from(spine_name::MAX_RECORD_TTL_SECS) + 1;
        let error = verify_resolution_at(&uri.to_string(), answer(&record), later).unwrap_err();
        assert!(matches!(
            outcome_error(&error),
            NameOutcomeError::InvalidEvidence { .. }
        ));
        // A signature that is not the key's.
        let mut forged = tombstone(&inception, &uri, 2, NOW);
        forged.signature = vec![0; 64];
        assert!(verify_resolution_at(&uri.to_string(), answer(&forged), NOW).is_err());
    }

    #[test]
    fn answers_without_evidence_pass_through() {
        for answer in [
            NameResolution::Unchanged { ttl_secs: 5 },
            NameResolution::NotFound { name: "n".into() },
            NameResolution::Invalid {
                name: "n".into(),
                reason: "r".into(),
            },
            NameResolution::Unavailable {
                name: "n".into(),
                reason: "r".into(),
            },
        ] {
            assert!(verify_resolution_at("spine://anything", answer, NOW).is_ok());
        }
        let error = VerifiedResolution::NotFound { name: "n".into() }
            .into_record("n")
            .unwrap_err();
        assert!(error.to_string().contains("did not resolve"));
    }

    #[test]
    fn the_callers_log_and_the_nodes_agree_or_the_longer_wins() {
        let rotated = Rotated::new(160);
        let ik = public(&rotated.inception);
        let empty = SuccessionLog::default();
        // The rotation after the first one: the first entry committed to
        // `rotated.next`, which is revealed now.
        let longer = rotated
            .log
            .rotated(
                &ik,
                &rotated.current,
                &public(&rotated.next),
                &public(&key(163)),
                NOW,
            )
            .unwrap();
        let name = rotated.name.clone();
        let combined = |node: &SuccessionLog, caller: &SuccessionLog| {
            combine_logs(&name, &ik, node, caller).unwrap()
        };
        assert_eq!(combined(&rotated.log, &empty), rotated.log);
        assert_eq!(combined(&empty, &rotated.log), rotated.log);
        assert_eq!(combined(&rotated.log, &rotated.log), rotated.log);
        assert_eq!(combined(&rotated.log, &longer), longer);
        assert_eq!(combined(&longer, &rotated.log), longer);
        // A conflicting log is a compromise.
        let other = SuccessionLog::default()
            .rotated(
                &ik,
                &rotated.inception,
                &public(&key(170)),
                &public(&key(171)),
                NOW,
            )
            .unwrap();
        let error = combine_logs(&name, &ik, &rotated.log, &other).unwrap_err();
        assert_eq!(
            outcome_error(&error),
            &NameOutcomeError::Compromised { name: name.clone() }
        );
    }

    /// A log that does not verify is refused, whichever side it came from.
    #[test]
    fn a_log_that_does_not_verify_cannot_be_combined() {
        let rotated = Rotated::new(180);
        let ik = public(&rotated.inception);
        let mut broken = rotated.log.clone();
        broken.entries[0].signature = vec![0; 64];
        let error = combine_logs(&rotated.name, &ik, &rotated.log, &broken).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<NameError>(),
            Some(NameError::InvalidSuccessionLog(_))
        ));
    }
}
