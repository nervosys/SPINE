//! Durable backing for the mesh name service's succession log book.
//!
//! [`crate::naming::NameService`] keeps, beside the replica records it holds
//! for others, a [`spine_name::LogBook`]: the best verified succession log of
//! each name it has heard of and every equivocation proof (spec 02, section
//! 10). The records are in memory by design. The book is the part that must
//! outlive a restart, because it is what refuses a superseded key, a retired
//! name and a compromised name: a node that forgets it accepts them again from
//! any peer.
//!
//! `spine-mesh` has no storage dependency, so the backend is a small trait the
//! embedding daemon implements (`spine-core` does, over its
//! `StorageBackend`). The trait carries typed values; the rules (verify every
//! row from its inception key, proofs before logs, the bounds, write before
//! apply) are applied by the service, not by the implementation.

use spine_name::{Equivocation, SuccessionLog};

/// A storage failure, as text: the service does not act on its kind.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct LogStoreError(pub String);

/// What a [`LogBookStore`] holds, as read at start-up. Nothing here is trusted:
/// the service verifies every row.
#[derive(Debug, Default)]
pub struct LogBookRows {
    /// Persisted equivocation proofs, by inception key.
    pub proofs: Vec<([u8; 32], Equivocation)>,
    /// Persisted succession logs, by inception key.
    pub logs: Vec<([u8; 32], SuccessionLog)>,
    /// Rows the implementation could not decode (a key that is not 32 bytes, a
    /// value that is not the JSON of a log or proof). They are counted as
    /// dropped by the service. An implementation that knows the raw key of such
    /// a row deletes it as it loads, so the next start does not count it again.
    pub unreadable: usize,
    /// How many of the `unreadable` rows the implementation tried to delete and
    /// could not. The service counts them in `log_store_failures`.
    pub delete_failures: usize,
}

/// A durable home for the log book of one [`crate::naming::NameService`].
///
/// Calls are synchronous and are made with the service locked, as the local
/// resolver's are: an implementation must not block for long or call back into
/// the service. A write that fails must return the error; the service then
/// refuses the offer that caused it and changes nothing.
pub trait LogBookStore: Send + Sync + std::fmt::Debug {
    /// Every stored row.
    fn load(&self) -> Result<LogBookRows, LogStoreError>;
    /// Store `log` under `inception_key`, replacing any earlier row.
    fn save_log(&self, inception_key: &[u8; 32], log: &SuccessionLog) -> Result<(), LogStoreError>;
    /// Store `proof` under `inception_key`.
    fn save_proof(
        &self,
        inception_key: &[u8; 32],
        proof: &Equivocation,
    ) -> Result<(), LogStoreError>;
    /// Delete the log row of `inception_key`, if any (an evicted log, or the log
    /// a proof replaces).
    fn remove_log(&self, inception_key: &[u8; 32]) -> Result<(), LogStoreError>;
    /// Delete the proof row of `inception_key`, if any (a proof row that did not
    /// verify or fit at load).
    fn remove_proof(&self, inception_key: &[u8; 32]) -> Result<(), LogStoreError>;
}

/// An in-memory store, with a switch that makes every write fail, for tests.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct MemoryLogBookStore {
    pub(crate) proofs: std::sync::Mutex<std::collections::HashMap<[u8; 32], Equivocation>>,
    pub(crate) logs: std::sync::Mutex<std::collections::HashMap<[u8; 32], SuccessionLog>>,
    /// Extra undecodable rows reported by `load`, which deletes them as a real
    /// store does: a second `load` reports none, unless writes are failing.
    pub(crate) unreadable: std::sync::atomic::AtomicUsize,
    /// While set, `save_*` and `remove_*` fail.
    pub(crate) failing: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl MemoryLogBookStore {
    pub(crate) fn fail_writes(&self, on: bool) {
        self.failing.store(on, std::sync::atomic::Ordering::SeqCst);
    }

    fn check(&self) -> Result<(), LogStoreError> {
        if self.failing.load(std::sync::atomic::Ordering::SeqCst) {
            Err(LogStoreError("disk full".into()))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
impl LogBookStore for MemoryLogBookStore {
    fn load(&self) -> Result<LogBookRows, LogStoreError> {
        let unreadable = self.unreadable.load(std::sync::atomic::Ordering::SeqCst);
        let delete_failures = if self.check().is_err() {
            unreadable
        } else {
            self.unreadable
                .store(0, std::sync::atomic::Ordering::SeqCst);
            0
        };
        Ok(LogBookRows {
            proofs: self
                .proofs
                .lock()
                .unwrap()
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            logs: self
                .logs
                .lock()
                .unwrap()
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            unreadable,
            delete_failures,
        })
    }

    fn save_log(&self, inception_key: &[u8; 32], log: &SuccessionLog) -> Result<(), LogStoreError> {
        self.check()?;
        self.logs
            .lock()
            .unwrap()
            .insert(*inception_key, log.clone());
        Ok(())
    }

    fn save_proof(
        &self,
        inception_key: &[u8; 32],
        proof: &Equivocation,
    ) -> Result<(), LogStoreError> {
        self.check()?;
        self.proofs
            .lock()
            .unwrap()
            .insert(*inception_key, proof.clone());
        Ok(())
    }

    fn remove_log(&self, inception_key: &[u8; 32]) -> Result<(), LogStoreError> {
        self.check()?;
        self.logs.lock().unwrap().remove(inception_key);
        Ok(())
    }

    fn remove_proof(&self, inception_key: &[u8; 32]) -> Result<(), LogStoreError> {
        self.check()?;
        self.proofs.lock().unwrap().remove(inception_key);
        Ok(())
    }
}
