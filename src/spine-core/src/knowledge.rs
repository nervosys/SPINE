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
    fn a_query_is_bounded_whatever_the_limit() {
        assert_eq!(query_limit(0), 0);
        assert_eq!(query_limit(10), 10);
        assert_eq!(query_limit(MAX_KNOWLEDGE_QUERY_RESULTS), 128);
        assert_eq!(query_limit(usize::MAX), MAX_KNOWLEDGE_QUERY_RESULTS);
    }
}
