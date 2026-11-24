//! What the daemon answers when a name resolves, and the evidence it attaches
//! (`docs/spec/00-wire.md` 12.5, `docs/spec/02-names.md` 10.10 decision 53).
//!
//! The pure half of the daemon's `resolve_one`: it maps what the local resolver
//! says (a record, or a [`NameError`]) onto a [`NameResolution`]. The node never
//! asks the client to trust an outcome. `resolved` carries the succession log
//! the node holds, `retired` the log that ends in a `retire` entry,
//! `compromised` the two logs of the equivocation proof, `revoked` the
//! tombstone and the log it is judged under; the client verifies each from the
//! name's inception key (`spine-agent`, `names.rs`).
//!
//! A node that holds a record of a rotated name but not the log that record
//! needs (the record's `spine.succession` head claim reaches past the held
//! log) does not answer `resolved` with a short log. It answers
//! [`NameResolution::Unavailable`], the outcome that already means "this node
//! cannot give an authoritative answer", with a reason that names the missing
//! log. `not_found` would be a lie in the other direction: a record exists.

use serde_json::Value;
use spine_name::{
    LocalResolver, NameError, NameRecord, Provenance, Resolution, SpineUri, SuccessionLog,
    SUCCESSION_META_KEY,
};
use spine_protocol::{NameProvenance, NameResolution};

/// Map a resolver provenance onto its protocol-level counterpart.
pub fn wire_provenance(p: Provenance) -> NameProvenance {
    match p {
        Provenance::Cache => NameProvenance::Cache,
        Provenance::StaleCache => NameProvenance::StaleCache,
        Provenance::Local => NameProvenance::Local,
        Provenance::Network => NameProvenance::Network,
        Provenance::Address => NameProvenance::Address,
    }
}

/// A log as the JSON-shaped entries the wire carries (`02-names.md` 10.1).
pub fn log_entries(log: &SuccessionLog) -> Result<Vec<Value>, serde_json::Error> {
    match serde_json::to_value(log)? {
        Value::Array(entries) => Ok(entries),
        // A log is `#[serde(transparent)]` over its entries: always an array.
        _ => Ok(Vec::new()),
    }
}

fn unavailable(raw: &str, reason: impl std::fmt::Display) -> NameResolution {
    NameResolution::Unavailable {
        name: raw.to_string(),
        reason: reason.to_string(),
    }
}

/// The succession log this node holds for `uri`'s name, as wire entries; empty
/// when it holds none (or the name has no key).
fn held_log(names: &LocalResolver, uri: &SpineUri) -> Result<Vec<Value>, serde_json::Error> {
    match uri.public_key().and_then(|key| names.succession_log(key)) {
        Some(log) => log_entries(&log),
        None => Ok(Vec::new()),
    }
}

/// The answer for a record the node is about to hand out: `resolved`, or
/// `unavailable` when the record needs a succession log the node does not hold.
///
/// A record that carries no `spine.succession` head claim is judged on its
/// signature alone, exactly as before, and its log is whatever the node holds
/// (empty for a name that never rotated). A record that does carry a claim is
/// judged against the held log with [`LocalResolver::authorise`], read-only: if
/// that refuses it for want of the log, or for any other succession reason, the
/// node does not present it as resolved.
pub fn resolved(
    names: &LocalResolver,
    raw: &str,
    record: &NameRecord,
    provenance: Provenance,
) -> NameResolution {
    if record.meta.contains_key(SUCCESSION_META_KEY) {
        if let Err(error) = names.authorise(record) {
            return unavailable(
                raw,
                format!("the record cannot be authorised by the succession log this node holds: {error}"),
            );
        }
    }
    let (record_json, log) = match (serde_json::to_value(record), held_log(names, &record.name)) {
        (Ok(record), Ok(log)) => (record, log),
        (Err(error), _) | (_, Err(error)) => {
            return NameResolution::Invalid {
                name: raw.to_string(),
                reason: error.to_string(),
            }
        }
    };
    let answer = NameResolution::Resolved {
        record: record_json,
        provenance: wire_provenance(provenance),
        log,
    };
    checked(raw, answer)
}

/// The answer for a tombstone: `revoked`, with the log the node holds.
pub fn revoked(
    names: &LocalResolver,
    raw: &str,
    record: &NameRecord,
    provenance: Provenance,
) -> NameResolution {
    let (record_json, log) = match (serde_json::to_value(record), held_log(names, &record.name)) {
        (Ok(record), Ok(log)) => (record, log),
        (Err(error), _) | (_, Err(error)) => {
            return NameResolution::Invalid {
                name: raw.to_string(),
                reason: error.to_string(),
            }
        }
    };
    checked(
        raw,
        NameResolution::Revoked {
            record: record_json,
            provenance: wire_provenance(provenance),
            log,
        },
    )
}

/// An answer the wire allows: one that breaks a bound (a log of more than 64
/// entries cannot be held, but the check is the sender's duty) is `unavailable`.
fn checked(raw: &str, answer: NameResolution) -> NameResolution {
    match answer.validate() {
        Ok(()) => answer,
        Err(reason) => unavailable(raw, format!("the node's answer is out of bounds: {reason}")),
    }
}

/// Map a refusal of the local resolver, or of the admission of a record found
/// on the mesh, onto an outcome that carries its evidence. `None` for an error
/// that is not about the name's standing (a malformed name, a `cap:` name):
/// the caller reports those as `invalid`, as before.
///
/// * [`NameError::Retired`]: `retired` with the held log, whose last entry is the
///   `retire`.
/// * [`NameError::Equivocated`]: `compromised` with the two logs of the held proof
///   ([`spine_name::Equivocation::logs`]).
/// * [`NameError::Revoked`]: `revoked` with the held tombstone.
/// * [`NameError::NeedSuccessionLog`] and [`NameError::SupersededKey`]: the record
///   the node has found cannot be authorised, so `unavailable`.
pub fn refusal(
    names: &LocalResolver,
    raw: &str,
    uri: &SpineUri,
    error: &NameError,
) -> Option<NameResolution> {
    match error {
        NameError::Retired(_) => {
            let log =
                match held_log(names, uri) {
                    Ok(log) if !log.is_empty() => log,
                    _ => return Some(unavailable(
                        raw,
                        "the name is retired but this node cannot produce the log that retires it",
                    )),
                };
            Some(checked(
                raw,
                NameResolution::Retired {
                    name: raw.to_string(),
                    log,
                },
            ))
        }
        NameError::Equivocated(_) => {
            let logs = uri
                .public_key()
                .and_then(|key| names.equivocation(key))
                .and_then(|proof| proof.logs());
            let Some([first, second]) = logs else {
                return Some(unavailable(
                    raw,
                    "the name is compromised but this node cannot produce the two logs of the proof",
                ));
            };
            let both = log_entries(&first).and_then(|a| log_entries(&second).map(|b| vec![a, b]));
            Some(match both {
                Ok(logs) => checked(
                    raw,
                    NameResolution::Compromised {
                        name: raw.to_string(),
                        logs,
                    },
                ),
                Err(error) => unavailable(raw, error),
            })
        }
        NameError::Revoked(_) => Some(match names.tombstone(uri) {
            Some(Resolution { record, provenance }) => revoked(names, raw, &record, provenance),
            None => unavailable(
                raw,
                "the name is revoked but its tombstone lapsed meanwhile",
            ),
        }),
        NameError::NeedSuccessionLog { .. } | NameError::SupersededKey(_) => Some(unavailable(
            raw,
            format!("a record was found that this node cannot authorise: {error}"),
        )),
        _ => None,
    }
}

/// The answer for a record found on the mesh, after the daemon offered the
/// log that came with it to the local resolver and tried to cache the record.
/// `cache_error` is what caching said. A refusal that is about the name's
/// standing (the log retires it, it is compromised, the record needs a log the
/// node lacks) becomes its outcome; a tombstone is `revoked`; anything else is
/// `resolved` with the log the node now holds, as before.
pub fn from_mesh(
    names: &LocalResolver,
    raw: &str,
    uri: &SpineUri,
    record: &NameRecord,
    cache_error: Option<&NameError>,
) -> NameResolution {
    if let Some(outcome) = cache_error.and_then(|error| refusal(names, raw, uri, error)) {
        return outcome;
    }
    if record.is_tombstone() {
        return revoked(names, raw, record, Provenance::Network);
    }
    resolved(names, raw, record, Provenance::Network)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use spine_name::{Endpoint, Resolver, SuccessionState};

    const NOW: u64 = 1_700_000_000;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn public(key: &SigningKey) -> [u8; 32] {
        key.verifying_key().to_bytes()
    }

    /// A name rotated once: `seed` is the inception key, `seed + 1` current and
    /// `seed + 2` the key the rotation commits to.
    struct Rotated {
        name: SpineUri,
        inception: SigningKey,
        current: SigningKey,
        log: SuccessionLog,
    }

    impl Rotated {
        fn new(seed: u8) -> Self {
            let inception = key(seed);
            let current = key(seed + 1);
            let log = SuccessionLog::default()
                .rotated(
                    &public(&inception),
                    &inception,
                    &public(&current),
                    &public(&key(seed + 2)),
                    NOW - 60,
                )
                .unwrap();
            Self {
                name: SpineUri::did(public(&inception)),
                inception,
                current,
                log,
            }
        }

        fn state(&self) -> SuccessionState {
            self.log.verify(&public(&self.inception)).unwrap()
        }

        fn record(&self, seq: u64) -> NameRecord {
            let mut record = NameRecord::new(self.name.clone(), seq, NOW)
                .unwrap()
                .with_endpoint(Endpoint::new("tcp", "127.0.0.1:9440"));
            record.sign_current(&self.current, &self.state()).unwrap();
            record
        }
    }

    fn resolver() -> LocalResolver {
        LocalResolver::at_time(NOW)
    }

    fn plain_record(seed: u8) -> NameRecord {
        let key = key(seed);
        let mut record = NameRecord::new(SpineUri::did(public(&key)), 1, NOW)
            .unwrap()
            .with_endpoint(Endpoint::new("tcp", "127.0.0.1:9440"));
        record.sign(&key).unwrap();
        record
    }

    #[test]
    fn a_name_that_never_rotated_resolves_with_an_empty_log() {
        let names = resolver();
        let record = plain_record(1);
        names.publish(record.clone()).unwrap();
        let answer = resolved(&names, "n", &record, Provenance::Local);
        let NameResolution::Resolved {
            log, provenance, ..
        } = answer
        else {
            panic!("{answer:?}")
        };
        assert!(log.is_empty());
        assert_eq!(provenance, NameProvenance::Local);
    }

    #[test]
    fn a_rotated_name_resolves_with_the_log_the_node_holds() {
        let names = resolver();
        let rotated = Rotated::new(10);
        let record = rotated.record(2);
        names
            .publish_with_log(record.clone(), rotated.log.clone())
            .unwrap();
        let answer = resolved(&names, "n", &record, Provenance::Local);
        let NameResolution::Resolved { log, .. } = answer else {
            panic!("{answer:?}")
        };
        assert_eq!(log, log_entries(&rotated.log).unwrap());
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn a_rotated_record_without_its_log_is_unavailable_not_resolved() {
        // The node holds the record (here: it was handed one) but not the log
        // its head claim needs. An empty log would read as "this name has
        // none"; the truthful answer is that the node cannot vouch.
        let names = resolver();
        let rotated = Rotated::new(20);
        let record = rotated.record(2);
        assert!(
            names.publish(record.clone()).is_err(),
            "the store refuses it too"
        );
        let answer = resolved(&names, "n", &record, Provenance::Network);
        match answer {
            NameResolution::Unavailable { name, reason } => {
                assert_eq!(name, "n");
                assert!(reason.contains("succession"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_retired_name_answers_with_the_log_that_retires_it() {
        let names = resolver();
        let rotated = Rotated::new(30);
        let retired = rotated
            .log
            .retired(
                &public(&rotated.inception),
                &rotated.current,
                &public(&key(32)),
                NOW,
            )
            .unwrap();
        names.offer_log(retired.clone()).unwrap();
        let uri = rotated.name.clone();
        let error = names.resolve(&uri).await.unwrap_err();
        let answer = refusal(&names, "n", &uri, &error).expect("retired is an outcome");
        let NameResolution::Retired { name, log } = answer else {
            panic!("{answer:?}")
        };
        assert_eq!(name, "n");
        assert_eq!(log, log_entries(&retired).unwrap());
        assert_eq!(log.last().unwrap()["kind"], "retire");
    }

    #[tokio::test]
    async fn an_equivocated_name_answers_with_the_two_logs_of_the_proof() {
        let names = resolver();
        let inception = key(40);
        let one = SuccessionLog::default()
            .rotated(
                &public(&inception),
                &inception,
                &public(&key(41)),
                &public(&key(42)),
                NOW,
            )
            .unwrap();
        let other = SuccessionLog::default()
            .rotated(
                &public(&inception),
                &inception,
                &public(&key(43)),
                &public(&key(44)),
                NOW,
            )
            .unwrap();
        names.offer_log(one.clone()).unwrap();
        names.offer_log(other.clone()).unwrap();
        let uri = SpineUri::did(public(&inception));
        let error = names.resolve(&uri).await.unwrap_err();
        assert!(matches!(error, NameError::Equivocated(_)));
        let answer = refusal(&names, "n", &uri, &error).expect("compromised is an outcome");
        let NameResolution::Compromised { logs, .. } = answer else {
            panic!("{answer:?}")
        };
        assert_eq!(logs.len(), 2);
        let sent: Vec<Vec<Value>> = vec![log_entries(&one).unwrap(), log_entries(&other).unwrap()];
        assert!(
            logs == sent || logs == vec![sent[1].clone(), sent[0].clone()],
            "the proof's logs are the two logs offered: {logs:?}"
        );
        answer_is_valid(&NameResolution::Compromised {
            name: "n".into(),
            logs,
        });
    }

    #[tokio::test]
    async fn a_revoked_name_answers_with_its_tombstone() {
        let names = resolver();
        let record = plain_record(50);
        names.publish(record.clone()).unwrap();
        let signing = key(50);
        let mut tombstone = NameRecord::tombstone(record.name.clone(), 2, NOW).unwrap();
        tombstone.sign(&signing).unwrap();
        names.publish(tombstone.clone()).unwrap();
        let uri = record.name.clone();
        let error = names.resolve(&uri).await.unwrap_err();
        assert!(matches!(error, NameError::Revoked(_)));
        let answer = refusal(&names, "n", &uri, &error).expect("revoked is an outcome");
        let NameResolution::Revoked { record, log, .. } = answer else {
            panic!("{answer:?}")
        };
        assert_eq!(record, serde_json::to_value(&tombstone).unwrap());
        assert!(log.is_empty(), "a name that never rotated has no log");
    }

    #[test]
    fn errors_that_are_not_about_standing_are_left_to_the_caller() {
        let names = resolver();
        let uri = SpineUri::did([9; 32]);
        for error in [
            NameError::NotFound("n".into()),
            NameError::NotASingleName("n".into()),
            NameError::BadSignature,
        ] {
            assert!(refusal(&names, "n", &uri, &error).is_none(), "{error}");
        }
    }

    #[test]
    fn a_mesh_record_the_node_cannot_authorise_is_unavailable() {
        let names = resolver();
        let rotated = Rotated::new(60);
        let record = rotated.record(2);
        let uri = record.name.clone();
        // The mesh sent the record without a usable log: caching it fails.
        let refused = names.cache_record(record.clone()).unwrap_err();
        assert!(matches!(refused, NameError::NeedSuccessionLog { .. }));
        let answer = from_mesh(&names, "n", &uri, &record, Some(&refused));
        assert!(
            matches!(answer, NameResolution::Unavailable { .. }),
            "{answer:?}"
        );
        // With the log offered first, caching succeeds and the answer carries it.
        names.offer_log(rotated.log.clone()).unwrap();
        let cached = names.cache_record(record.clone());
        assert!(cached.is_ok());
        let answer = from_mesh(&names, "n", &uri, &record, cached.as_ref().err());
        let NameResolution::Resolved {
            log, provenance, ..
        } = answer
        else {
            panic!("{answer:?}")
        };
        assert_eq!(log.len(), 1);
        assert_eq!(provenance, NameProvenance::Network);
    }

    #[test]
    fn a_tombstone_found_on_the_mesh_is_revoked() {
        let names = resolver();
        let signing = key(70);
        let mut tombstone = NameRecord::tombstone(SpineUri::did(public(&signing)), 2, NOW).unwrap();
        tombstone.sign(&signing).unwrap();
        let uri = tombstone.name.clone();
        names.cache_record(tombstone.clone()).unwrap();
        let answer = from_mesh(&names, "n", &uri, &tombstone, None);
        let NameResolution::Revoked { provenance, .. } = answer else {
            panic!("{answer:?}")
        };
        assert_eq!(provenance, NameProvenance::Network);
    }

    fn answer_is_valid(answer: &NameResolution) {
        answer.validate().unwrap();
    }
}
