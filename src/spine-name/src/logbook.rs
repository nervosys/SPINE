//! The succession logs a verifier holds, and the admission check built on them.
//!
//! [`crate::succession`] defines logs, their verification and the pure function
//! [`record_authorised`]. This module is the state around it: a [`LogBook`]
//! keeps, for each name (by inception key), the best verified log, and every
//! equivocation proof, within fixed bounds; [`LogBook::authorise`] is the single
//! admission check that [`crate::RecordStore`], [`crate::LocalResolver`] and the
//! mesh call in place of [`NameRecord::verify`].
//!
//! A name with no log behaves exactly as before: with an empty book, and a
//! record that carries no `spine.succession` claim, `authorise` *is*
//! `NameRecord::verify`, with no lookup, no allocation and no I/O.
//!
//! # Bounds
//!
//! At most [`MAX_SUCCESSION_LOGS`] logs and [`MAX_EQUIVOCATION_PROOFS`] proofs
//! are kept. A log is replaced only by a valid strict extension of itself. A
//! log is never evicted while the store holds a record of its name (the
//! caller passes the set of such names); when the table is full and every log
//! has records held, a new log is refused ([`LogOffer::Full`]). A log without
//! held records is evictable, non-retired logs first and then oldest first: the
//! consequence is that the verifier forgets that rotation (or retirement) and
//! judges the name by its inception key until it learns the log again. Proofs
//! are never evicted and never cleared; when the proof table is full a
//! conflicting log is refused and not adopted ([`LogOffer::ProofsFull`]), so
//! the name keeps the log it already had and does *not* fail closed here.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::record::NameRecord;
use crate::succession::{
    record_authorised, Equivocation, RecordAuthority, Refusal, State, SuccessionError,
    SuccessionLog, MAX_SUCCESSION_ENTRIES, SUCCESSION_META_KEY,
};
use crate::uri::SpineUri;
use crate::NameError;

/// Most succession logs one [`LogBook`] keeps. A log is at most
/// [`MAX_SUCCESSION_ENTRIES`] entries (about 200 bytes each), so the book is
/// bounded at roughly 13 MiB.
pub const MAX_SUCCESSION_LOGS: usize = 1024;

/// Most equivocation proofs one [`LogBook`] keeps. A proof carries a prefix of
/// at most 63 entries and two more.
pub const MAX_EQUIVOCATION_PROOFS: usize = 256;

/// The field of a publication object (the JSON a `name publish` sends) that
/// carries the succession log next to the record's own fields.
pub const PUBLICATION_LOG_FIELD: &str = "succession_log";

/// Judge `record` against the state of the verifier's best log for its name
/// (`None`: no log), as an admission check returning a [`NameError`].
///
/// [`RecordAuthority::Authorised`] is `Ok`; [`RecordAuthority::NeedLog`] is
/// [`NameError::NeedSuccessionLog`] (the caller must obtain the log);
/// a retired name is [`NameError::Retired`], a superseded key
/// [`NameError::SupersededKey`], a bad signature or a record-rule failure the
/// same error `verify` gives, and any other refusal
/// [`NameError::SuccessionRefused`].
pub fn authorise_record(record: &NameRecord, state: Option<&State>) -> Result<(), NameError> {
    let name = || record.name.to_string();
    match record_authorised(record, state) {
        RecordAuthority::Authorised => Ok(()),
        RecordAuthority::NeedLog { claimed, held } => Err(NameError::NeedSuccessionLog {
            name: name(),
            claimed: claimed.index,
            held,
        }),
        RecordAuthority::Refused(refusal) => Err(match refusal {
            Refusal::Retired => NameError::Retired(name()),
            Refusal::SupersededKey => NameError::SupersededKey(name()),
            Refusal::BadSignature => NameError::BadSignature,
            Refusal::Record(error) => error,
            other => NameError::SuccessionRefused {
                name: name(),
                reason: other.reason().to_string(),
            },
        }),
    }
}

fn has_claim(record: &NameRecord) -> bool {
    record.meta.contains_key(SUCCESSION_META_KEY)
}

/// What [`LogBook::plan`] decided about an offered log. Nothing has changed in
/// the book yet: [`LogBook::apply`] performs it, so a caller can make the
/// matching durable write first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogOffer {
    /// The log is empty, equal to the held one, or a prefix of it: nothing to
    /// learn.
    Unchanged,
    /// The name already has an equivocation proof; the log is not needed.
    AlreadyCompromised,
    /// The log is the first for its name or a valid strict extension of the
    /// held one. `evict` is a log without held records that makes room.
    Adopt {
        inception_key: [u8; 32],
        log: SuccessionLog,
        /// The state the log verified to (boxed to keep the enum small).
        state: Box<State>,
        evict: Option<[u8; 32]>,
    },
    /// The log is valid and conflicts with the held one: this is the proof.
    Conflict(Box<Equivocation>),
    /// A new log, but the table is full and every log has records held.
    Full,
    /// A conflicting log, but the proof table is full.
    ProofsFull,
}

/// The result of [`LogBook::apply`], for the layer that holds records to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogChange {
    /// Nothing changed.
    Unchanged,
    /// The log for `inception_key` was adopted (or extended); records of that
    /// name may no longer be authorised.
    Adopted { inception_key: [u8; 32] },
    /// An equivocation proof was recorded for `inception_key`; no record of the
    /// name is authorised any more.
    Compromised { inception_key: [u8; 32] },
}

#[derive(Debug, Clone)]
struct Held {
    log: SuccessionLog,
    state: State,
    stamp: u64,
}

/// The verified succession logs and equivocation proofs of one verifier. See the
/// module documentation for the bounds and the eviction rule.
#[derive(Debug, Clone, Default)]
pub struct LogBook {
    logs: HashMap<[u8; 32], Held>,
    proofs: HashMap<[u8; 32], Equivocation>,
    counter: u64,
}

impl LogBook {
    /// An empty book: every name is judged by its inception key.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the book holds no log and no proof, which is the state in which
    /// [`LogBook::authorise`] is exactly [`NameRecord::verify`].
    pub fn is_empty(&self) -> bool {
        self.logs.is_empty() && self.proofs.is_empty()
    }

    /// Logs held.
    pub fn log_count(&self) -> usize {
        self.logs.len()
    }

    /// Equivocation proofs held.
    pub fn proof_count(&self) -> usize {
        self.proofs.len()
    }

    /// Whether the log table is at its bound.
    pub fn is_full(&self) -> bool {
        self.logs.len() >= MAX_SUCCESSION_LOGS
    }

    /// The state of the best log held for a name, if any.
    pub fn state(&self, inception_key: &[u8; 32]) -> Option<&State> {
        self.logs.get(inception_key).map(|h| &h.state)
    }

    /// The best log held for a name, if any.
    pub fn log(&self, inception_key: &[u8; 32]) -> Option<&SuccessionLog> {
        self.logs.get(inception_key).map(|h| &h.log)
    }

    /// The equivocation proof held for a name, if any.
    pub fn proof(&self, inception_key: &[u8; 32]) -> Option<&Equivocation> {
        self.proofs.get(inception_key)
    }

    /// The logs to send a peer that asks about a name: the best log held, or, for
    /// a name with a proof, the proof's two logs (the held log was replaced by
    /// the proof, decision 29, and the two conflicting logs are what a receiver
    /// needs to derive the proof again). Empty for a name with neither, and at
    /// once, without any lookup, when the book is empty.
    pub fn logs_to_send(&self, inception_key: &[u8; 32]) -> Vec<SuccessionLog> {
        if self.is_empty() {
            return Vec::new();
        }
        if let Some(proof) = self.proofs.get(inception_key) {
            return proof.logs().map(Vec::from).unwrap_or_default();
        }
        self.logs
            .get(inception_key)
            .map(|held| vec![held.log.clone()])
            .unwrap_or_default()
    }

    /// Whether a proof is held: the name fails closed.
    pub fn is_compromised(&self, inception_key: &[u8; 32]) -> bool {
        self.proofs.contains_key(inception_key)
    }

    /// Every held log, for persistence.
    pub fn logs(&self) -> impl Iterator<Item = (&[u8; 32], &SuccessionLog)> {
        self.logs.iter().map(|(k, h)| (k, &h.log))
    }

    /// Every held proof, for persistence.
    pub fn proofs(&self) -> impl Iterator<Item = (&[u8; 32], &Equivocation)> {
        self.proofs.iter()
    }

    /// The error a resolution of `uri` must answer with instead of a record or
    /// "not found": [`NameError::Equivocated`] if a proof is held,
    /// [`NameError::Retired`] if the best log retires the name. `None` for every
    /// other name, and at once, without hashing or lookup, when the book is empty.
    pub fn status_error(&self, uri: &SpineUri) -> Option<NameError> {
        if self.is_empty() {
            return None;
        }
        let key = uri.public_key()?;
        if self.proofs.contains_key(key) {
            return Some(NameError::Equivocated(uri.to_string()));
        }
        match self.logs.get(key) {
            Some(held) if held.state.is_retired() => Some(NameError::Retired(uri.to_string())),
            _ => None,
        }
    }

    /// The admission check: whether `record` may enter a store or cache that
    /// holds this book.
    ///
    /// * No log, no proof, and the record carries no `spine.succession` claim:
    ///   exactly [`NameRecord::verify`] (the path every name without a log takes).
    /// * A proof is held for the name: [`NameError::Equivocated`], whatever the
    ///   record.
    /// * Otherwise [`authorise_record`] against the held log's state (none: the
    ///   empty log, which still sends a record with a claim to
    ///   [`NameError::NeedSuccessionLog`]).
    pub fn authorise(&self, record: &NameRecord) -> Result<(), NameError> {
        let claim = has_claim(record);
        if self.is_empty() && !claim {
            return record.verify();
        }
        let Some(key) = record.name.public_key() else {
            return record.verify();
        };
        if self.proofs.contains_key(key) {
            return Err(NameError::Equivocated(record.name.to_string()));
        }
        let state = self.logs.get(key).map(|h| &h.state);
        if state.is_none() && !claim {
            return record.verify();
        }
        authorise_record(record, state)
    }

    /// Decide what to do with an offered log of the name whose inception key is
    /// `inception_key`, without changing anything. `protected` holds the
    /// inception keys of names the store holds records of: a log of such a name
    /// is never chosen for eviction.
    ///
    /// An empty log, a log equal to the held one or a prefix of it is
    /// [`LogOffer::Unchanged`] without any signature being checked (a prefix of
    /// a verified log is verified). Otherwise the log is verified from the
    /// inception key (an invalid log is an error), and then it is adopted if the
    /// name has none or if the held log is a prefix of it, and is a
    /// [`LogOffer::Conflict`] with the proof if neither is a prefix of the other.
    pub fn plan(
        &self,
        inception_key: &[u8; 32],
        log: SuccessionLog,
        protected: &HashSet<[u8; 32]>,
    ) -> Result<LogOffer, SuccessionError> {
        if log.entries.is_empty() {
            return Ok(LogOffer::Unchanged);
        }
        if self.proofs.contains_key(inception_key) {
            return Ok(LogOffer::AlreadyCompromised);
        }
        let held = self.logs.get(inception_key);
        if let Some(held) = held {
            let common = held.log.entries.len().min(log.entries.len());
            if log.entries.len() <= held.log.entries.len()
                && held.log.entries[..common] == log.entries[..common]
            {
                return Ok(LogOffer::Unchanged);
            }
        }
        let state = log.verify(inception_key)?;

        let Some(held) = held else {
            let evict = if self.is_full() {
                let Some(key) = self.evictable(protected) else {
                    return Ok(LogOffer::Full);
                };
                Some(key)
            } else {
                None
            };
            return Ok(LogOffer::Adopt {
                inception_key: *inception_key,
                log,
                state: Box::new(state),
                evict,
            });
        };

        let held_len = held.log.entries.len();
        if log.entries.len() > held_len && held.log.entries[..] == log.entries[..held_len] {
            return Ok(LogOffer::Adopt {
                inception_key: *inception_key,
                log,
                state: Box::new(state),
                evict: None,
            });
        }

        // Neither is a prefix of the other: both verify, so they diverge at the
        // first differing index, with a shared prefix and one signer.
        let Some(split) = held
            .log
            .entries
            .iter()
            .zip(log.entries.iter())
            .position(|(a, b)| a != b)
        else {
            return Ok(LogOffer::Unchanged);
        };
        let proof = Equivocation {
            prefix: SuccessionLog {
                entries: held.log.entries[..split].to_vec(),
            },
            first: held.log.entries[split].clone(),
            second: log.entries[split].clone(),
        };
        proof.verify(inception_key)?;
        if self.proofs.len() >= MAX_EQUIVOCATION_PROOFS {
            return Ok(LogOffer::ProofsFull);
        }
        Ok(LogOffer::Conflict(Box::new(proof)))
    }

    /// Carry out a plan from [`LogBook::plan`] made against this book.
    pub fn apply(&mut self, offer: LogOffer) -> LogChange {
        match offer {
            LogOffer::Unchanged
            | LogOffer::AlreadyCompromised
            | LogOffer::Full
            | LogOffer::ProofsFull => LogChange::Unchanged,
            LogOffer::Adopt {
                inception_key,
                log,
                state,
                evict,
            } => {
                if let Some(old) = evict {
                    self.logs.remove(&old);
                }
                self.counter += 1;
                self.logs.insert(
                    inception_key,
                    Held {
                        log,
                        state: *state,
                        stamp: self.counter,
                    },
                );
                LogChange::Adopted { inception_key }
            }
            LogOffer::Conflict(proof) => {
                let key = proof.first.inception_key;
                self.logs.remove(&key);
                self.proofs.insert(key, *proof);
                LogChange::Compromised { inception_key: key }
            }
        }
    }

    /// Restore a persisted log: it must verify from `inception_key`, and there
    /// must be room. Returns whether it was taken.
    pub fn restore_log(&mut self, inception_key: [u8; 32], log: SuccessionLog) -> bool {
        if log.entries.is_empty()
            || self.logs.len() >= MAX_SUCCESSION_LOGS
            || self.proofs.contains_key(&inception_key)
        {
            return false;
        }
        let Ok(state) = log.verify(&inception_key) else {
            return false;
        };
        self.counter += 1;
        self.logs.insert(
            inception_key,
            Held {
                log,
                state,
                stamp: self.counter,
            },
        );
        true
    }

    /// Restore a persisted proof: it must verify from `inception_key`, and there
    /// must be room. Returns whether it was taken.
    pub fn restore_proof(&mut self, inception_key: [u8; 32], proof: Equivocation) -> bool {
        if self.proofs.len() >= MAX_EQUIVOCATION_PROOFS
            || proof.first.inception_key != inception_key
            || proof.verify(&inception_key).is_err()
        {
            return false;
        }
        self.logs.remove(&inception_key);
        self.proofs.insert(inception_key, proof);
        true
    }

    /// The log to evict to make room: one whose name has no record held, a
    /// non-retired log before a retired one, and the oldest first.
    fn evictable(&self, protected: &HashSet<[u8; 32]>) -> Option<[u8; 32]> {
        self.logs
            .iter()
            .filter(|(key, _)| !protected.contains(*key))
            .min_by_key(|(_, held)| (held.state.is_retired(), held.stamp))
            .map(|(key, _)| *key)
    }
}

/// The inception key of the name a log is for: the key of its first entry.
/// `None` for an empty log.
pub fn log_inception_key(log: &SuccessionLog) -> Option<[u8; 32]> {
    log.entries.first().map(|e| e.inception_key)
}

/// Whether a log is within the bounds a receiver enforces before verifying it:
/// at most [`MAX_SUCCESSION_ENTRIES`] entries, each with a 64-byte signature.
pub fn log_within_bounds(log: &SuccessionLog) -> bool {
    log.entries.len() <= MAX_SUCCESSION_ENTRIES
        && log.entries.iter().all(|e| e.signature.len() == 64)
}

/// The JSON object `name publish` sends: the record's own fields and, beside
/// them under [`PUBLICATION_LOG_FIELD`], its succession log. A reader that does
/// not know the field reads the object as the plain record.
pub fn publication_value(
    record: &NameRecord,
    log: &SuccessionLog,
) -> Result<Value, serde_json::Error> {
    let mut value = serde_json::to_value(record)?;
    if let Value::Object(map) = &mut value {
        map.insert(
            PUBLICATION_LOG_FIELD.to_string(),
            serde_json::to_value(log)?,
        );
    }
    Ok(value)
}

/// The succession log in a publication object, if it carries one. `None` when
/// the field is absent; an error when it is present and not a log.
pub fn publication_log(value: &Value) -> Option<Result<SuccessionLog, serde_json::Error>> {
    value
        .get(PUBLICATION_LOG_FIELD)
        .map(|v| serde_json::from_value(v.clone()))
}
