//! Bounds on a session's knowledge entries, as the daemon applies them to the
//! direct `store_knowledge` and `query_knowledge` commands. Pure functions, so
//! the daemon (a binary) and the tests share one definition.
//!
//! The per-session entry count is `MAX_KNOWLEDGE_ENTRIES_PER_SESSION` in
//! `main.rs`, which the daemon passes in; `execute_binary` applies the same
//! count to the entries a program stores.

/// Largest knowledge entry the direct command stores: the length of the key,
/// of the compact JSON text of the value, and of every tag, summed. It equals
/// the 64 KiB a program may hold in one host value.
pub const MAX_KNOWLEDGE_ENTRY_BYTES: usize = 64 * 1024;

/// Most entries one `query_knowledge` returns, whatever `limit` asks. At the
/// entry size above this keeps the reply near 8 MiB, under the 16 MiB frame
/// limit.
pub const MAX_KNOWLEDGE_QUERY_RESULTS: usize = 128;

/// The size of an entry as bounded by [`MAX_KNOWLEDGE_ENTRY_BYTES`].
pub fn entry_bytes(key: &str, value: &serde_json::Value, tags: &[String]) -> usize {
    let value_len = serde_json::to_vec(value).map_or(usize::MAX, |text| text.len());
    tags.iter()
        .fold(key.len().saturating_add(value_len), |total, tag| {
            total.saturating_add(tag.len())
        })
}

/// Whether a session already holding `held` entries may store one more of
/// `size` bytes, against a cap of `max_entries`. The error is the reason, for
/// the message of the refusal.
pub fn admit_entry(held: usize, max_entries: usize, size: usize) -> Result<(), String> {
    if size > MAX_KNOWLEDGE_ENTRY_BYTES {
        return Err(format!(
            "a knowledge entry may hold at most {MAX_KNOWLEDGE_ENTRY_BYTES} bytes"
        ));
    }
    if held >= max_entries {
        return Err(format!(
            "a session may hold at most {max_entries} knowledge entries"
        ));
    }
    Ok(())
}

/// What one stored entry costs the daemon-wide budget beyond the bytes of
/// [`entry_bytes`]: the JSON field names, quotes, timestamp and separators the
/// snapshot writes around every entry, rounded up. It keeps a million tiny
/// entries from costing nothing.
pub const KNOWLEDGE_ENTRY_OVERHEAD_BYTES: u64 = 128;

/// Bytes of knowledge, summed over every session and the cluster buckets, the
/// daemon holds at most.
///
/// Per session the bounds are 1024 entries of 64 KiB, 64 MiB; with the default
/// `max_sessions` of 1000 (and detached sessions kept for 3600 s) that is
/// about 1000 x 1024 x 64 KiB = 62.5 GiB of memory and of snapshot. The
/// snapshot (`session_store::MAX_SNAPSHOT_BYTES`) is 64 MiB and holds the
/// sessions as well as the knowledge: a save that would pass it fails, the
/// previous snapshot stays, the daemon logs the error and reports
/// persistence as failing, and a start-up that finds a larger file refuses
/// to read it. This budget is half the snapshot bound, so the entries'
/// counted bytes, the per-entry overhead above and the JSON escaping of
/// typical text fit with room left for the sessions.
pub const MAX_KNOWLEDGE_BYTES: u64 = 32 * 1024 * 1024;

/// The charge of one entry of `size` bytes (as [`entry_bytes`] measures it).
pub fn entry_cost(size: usize) -> u64 {
    (size as u64).saturating_add(KNOWLEDGE_ENTRY_OVERHEAD_BYTES)
}

/// The daemon-wide byte budget for knowledge. Charges are reserved before an
/// entry is stored and released when it is removed, so the total never passes
/// the limit, however many threads reserve at once.
#[derive(Debug)]
pub struct KnowledgeBudget {
    used: std::sync::atomic::AtomicU64,
    limit: u64,
}

impl KnowledgeBudget {
    /// An empty budget of `limit` bytes.
    pub fn new(limit: u64) -> Self {
        Self {
            used: std::sync::atomic::AtomicU64::new(0),
            limit,
        }
    }

    /// Bytes charged now.
    pub fn used(&self) -> u64 {
        self.used.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Reserves `cost` bytes, or refuses with the reason when that would pass
    /// the limit. Nothing is reserved on a refusal.
    pub fn reserve(&self, cost: u64) -> Result<(), String> {
        self.update(|used| used.checked_add(cost).filter(|total| *total <= self.limit))
            .map_err(|()| {
                format!(
                    "the daemon may hold at most {} bytes of knowledge",
                    self.limit
                )
            })
    }

    /// Charges `cost` bytes without a limit check, for state already held
    /// (loaded from a snapshot): later reservations are refused until enough
    /// is released.
    pub fn charge(&self, cost: u64) {
        let _ = self.update(|used| Some(used.saturating_add(cost)));
    }

    /// Returns `cost` bytes, never below zero.
    pub fn release(&self, cost: u64) {
        let _ = self.update(|used| Some(used.saturating_sub(cost)));
    }

    /// Replaces the counter with `next(current)` atomically, retrying when
    /// another thread changed it in between; `Err` when `next` declines. A
    /// plain compare-and-swap loop: `fetch_update` is deprecated on current
    /// toolchains and its replacement is newer than the minimum supported one.
    fn update(&self, next: impl Fn(u64) -> Option<u64>) -> Result<(), ()> {
        use std::sync::atomic::Ordering;
        let mut current = self.used.load(Ordering::Acquire);
        loop {
            let Some(wanted) = next(current) else {
                return Err(());
            };
            match self.used.compare_exchange_weak(
                current,
                wanted,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(seen) => current = seen,
            }
        }
    }
}

/// The number of results a query returns for a requested `limit`.
pub fn query_limit(limit: usize) -> usize {
    limit.min(MAX_KNOWLEDGE_QUERY_RESULTS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_size_counts_key_value_and_tags() {
        let value = serde_json::json!("abc");
        // 3 (key) + 5 (`"abc"` with quotes) + 2 + 3 (tags).
        assert_eq!(
            entry_bytes("key", &value, &["ab".to_string(), "cde".to_string()]),
            13
        );
        assert_eq!(entry_bytes("", &serde_json::Value::Null, &[]), 4);
    }

    #[test]
    fn an_entry_is_refused_over_the_size_or_the_count() {
        assert!(admit_entry(0, 3, MAX_KNOWLEDGE_ENTRY_BYTES).is_ok());
        assert!(admit_entry(2, 3, 1).is_ok());
        assert!(admit_entry(3, 3, 1).unwrap_err().contains("at most 3"));
        assert!(admit_entry(0, 3, MAX_KNOWLEDGE_ENTRY_BYTES + 1)
            .unwrap_err()
            .contains("65536 bytes"));
        // The size is judged before the count, so one refusal names one cause.
        assert!(admit_entry(9, 3, MAX_KNOWLEDGE_ENTRY_BYTES + 1)
            .unwrap_err()
            .contains("bytes"));
    }

    #[test]
    fn the_budget_refuses_past_its_limit_and_release_restores_it() {
        let budget = KnowledgeBudget::new(100);
        assert!(budget.reserve(60).is_ok());
        assert!(budget.reserve(40).is_ok());
        assert_eq!(budget.used(), 100);
        let reason = budget.reserve(1).unwrap_err();
        assert!(reason.contains("at most 100 bytes"), "{reason}");
        // A refusal reserves nothing.
        assert_eq!(budget.used(), 100);
        budget.release(60);
        assert_eq!(budget.used(), 40);
        assert!(budget.reserve(60).is_ok());
        budget.release(100);
        assert_eq!(budget.used(), 0);
        // Release never goes below zero; a huge reservation does not wrap.
        budget.release(5);
        assert_eq!(budget.used(), 0);
        assert!(budget.reserve(u64::MAX).is_err());
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn loaded_state_over_the_limit_blocks_reservations_until_released() {
        let budget = KnowledgeBudget::new(100);
        budget.charge(150);
        assert_eq!(budget.used(), 150);
        assert!(budget.reserve(1).is_err());
        budget.release(100);
        assert!(budget.reserve(50).is_ok());
        assert_eq!(budget.used(), 100);
    }

    #[test]
    fn concurrent_reservations_never_pass_the_limit() {
        let budget = std::sync::Arc::new(KnowledgeBudget::new(1000));
        let granted = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let budget = budget.clone();
                let granted = granted.clone();
                std::thread::spawn(move || {
                    for _ in 0..500 {
                        if budget.reserve(7).is_ok() {
                            granted.fetch_add(7, std::sync::atomic::Ordering::SeqCst);
                        }
                        assert!(budget.used() <= 1000);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        // 1000 / 7 = 142 reservations fit; every one that did was counted.
        assert_eq!(
            budget.used(),
            granted.load(std::sync::atomic::Ordering::SeqCst)
        );
        assert_eq!(budget.used(), 142 * 7);
        // Releasing exactly what was granted restores an empty budget.
        for _ in 0..142 {
            budget.release(7);
        }
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn an_entry_costs_its_bytes_plus_the_overhead() {
        assert_eq!(entry_cost(0), KNOWLEDGE_ENTRY_OVERHEAD_BYTES);
        assert_eq!(entry_cost(10), 10 + KNOWLEDGE_ENTRY_OVERHEAD_BYTES);
        assert!(MAX_KNOWLEDGE_BYTES <= crate::session_store::MAX_SNAPSHOT_BYTES / 2);
    }

    #[test]
    fn a_query_is_bounded_whatever_the_limit() {
        assert_eq!(query_limit(0), 0);
        assert_eq!(query_limit(10), 10);
        assert_eq!(query_limit(MAX_KNOWLEDGE_QUERY_RESULTS), 128);
        assert_eq!(query_limit(usize::MAX), MAX_KNOWLEDGE_QUERY_RESULTS);
    }
}
