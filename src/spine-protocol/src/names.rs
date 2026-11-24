//! The succession evidence in a name resolution (`docs/spec/00-wire.md` 12.5).
//!
//! A `resolved` answer carries the name's succession log, and the outcomes
//! `retired`, `compromised` and `revoked` carry the evidence a client needs to
//! check them. This crate does not depend on `spine-name`, so the log travels
//! in the JSON form of `docs/spec/02-names.md` 10.1 as a JSON-shaped value, the
//! same convention a name record has in the same answer, and this module checks
//! structure only: the number of entries, and that each entry is a closed map of
//! the eight fields with the right kinds of value. Whether the signatures verify
//! and the keys chain is the client's business (`spine-agent`, which does depend
//! on `spine-name`); a decoder that read more here would have to repeat the
//! succession rules.

use serde::de::{self, Deserialize, Deserializer};
use serde_json::Value;

/// Most entries in one log on the wire (`spine_name::MAX_SUCCESSION_ENTRIES`).
pub const MAX_LOG_ENTRIES: usize = 64;

/// How many logs a `compromised` outcome carries: the two logs of an
/// equivocation proof (`docs/spec/02-names.md` 10.10 decision 51).
pub const COMPROMISED_LOGS: usize = 2;

/// The fields of one log entry, in the JSON form of `02-names.md` 10.1.
const ENTRY_KEYS: [&str; 8] = [
    "commitment",
    "effective_at",
    "inception_key",
    "index",
    "key",
    "kind",
    "prev",
    "signature",
];

/// The longest text of an entry field: a 64-byte signature is 103 symbols.
const MAX_ENTRY_TEXT: usize = 128;

fn entry_error(position: usize, why: &str) -> String {
    format!("log entry {position}: {why}")
}

/// Whether `value` is one log entry: a closed map of the eight fields, `index`
/// and `effective_at` unsigned integers, `inception_key`, `key` and `signature`
/// text, `prev` and `commitment` text or null, `kind` `rotate` or `retire`.
fn check_entry(position: usize, value: &Value) -> Result<(), String> {
    let Value::Object(map) = value else {
        return Err(entry_error(position, "not an object"));
    };
    if map.len() != ENTRY_KEYS.len() || !ENTRY_KEYS.iter().all(|key| map.contains_key(*key)) {
        return Err(entry_error(
            position,
            "not exactly the fields of a succession entry",
        ));
    }
    let short_text = |key: &str| {
        map.get(key)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty() && s.len() <= MAX_ENTRY_TEXT)
    };
    let short_text_or_null = |key: &str| match map.get(key) {
        Some(Value::Null) => true,
        Some(Value::String(s)) => !s.is_empty() && s.len() <= MAX_ENTRY_TEXT,
        _ => false,
    };
    let uint = |key: &str| map.get(key).is_some_and(Value::is_u64);
    if !(short_text("inception_key")
        && short_text("key")
        && short_text("signature")
        && short_text_or_null("prev")
        && short_text_or_null("commitment")
        && uint("index")
        && uint("effective_at"))
    {
        return Err(entry_error(position, "a field has the wrong type or size"));
    }
    match map.get("kind").and_then(Value::as_str) {
        Some("rotate" | "retire") => Ok(()),
        _ => Err(entry_error(position, "kind is not rotate or retire")),
    }
}

/// Check one log: between `min` and [`MAX_LOG_ENTRIES`] entries, each
/// well-formed.
pub fn check_log(entries: &[Value], min: usize) -> Result<(), String> {
    if entries.len() < min {
        return Err(format!(
            "a log of {} entries, at least {min} required",
            entries.len()
        ));
    }
    if entries.len() > MAX_LOG_ENTRIES {
        return Err(format!(
            "a log of {} entries, more than {MAX_LOG_ENTRIES}",
            entries.len()
        ));
    }
    entries
        .iter()
        .enumerate()
        .try_for_each(|(i, entry)| check_entry(i, entry))
}

/// Check the logs of a `compromised` outcome: exactly [`COMPROMISED_LOGS`], each
/// of 1 to [`MAX_LOG_ENTRIES`] entries.
pub fn check_compromised_logs(logs: &[Vec<Value>]) -> Result<(), String> {
    if logs.len() != COMPROMISED_LOGS {
        return Err(format!(
            "{} logs, exactly {COMPROMISED_LOGS} required",
            logs.len()
        ));
    }
    logs.iter().try_for_each(|log| check_log(log, 1))
}

/// `deserialize_with` for the `log` of `resolved` and `revoked`: 0 to 64 entries.
pub(crate) fn any_log<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Value>, D::Error> {
    let entries = Vec::<Value>::deserialize(deserializer)?;
    check_log(&entries, 0).map_err(de::Error::custom)?;
    Ok(entries)
}

/// `deserialize_with` for the `log` of `retired`: 1 to 64 entries.
pub(crate) fn non_empty_log<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Value>, D::Error> {
    let entries = Vec::<Value>::deserialize(deserializer)?;
    check_log(&entries, 1).map_err(de::Error::custom)?;
    Ok(entries)
}

/// `deserialize_with` for the `logs` of `compromised`: exactly two logs.
pub(crate) fn two_logs<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Vec<Value>>, D::Error> {
    let logs = Vec::<Vec<Value>>::deserialize(deserializer)?;
    check_compromised_logs(&logs).map_err(de::Error::custom)?;
    Ok(logs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// An entry of the right shape. Nothing here is a real signature: this
    /// module checks shape only.
    fn entry(index: u64) -> Value {
        let prev = if index == 1 {
            Value::Null
        } else {
            json!("b".repeat(52))
        };
        json!({
            "inception_key": "a".repeat(52),
            "index": index,
            "prev": prev,
            "kind": "rotate",
            "key": "c".repeat(52),
            "commitment": "d".repeat(52),
            "effective_at": 1_700_000_000u64,
            "signature": "e".repeat(103),
        })
    }

    fn log(n: usize) -> Vec<Value> {
        (1..=n as u64).map(entry).collect()
    }

    #[test]
    fn a_log_of_zero_to_sixty_four_entries_has_the_shape() {
        for n in [0, 1, 2, 63, 64] {
            check_log(&log(n), 0).unwrap_or_else(|e| panic!("{n}: {e}"));
        }
        assert!(check_log(&log(65), 0).is_err());
        assert!(check_log(&log(0), 1).is_err(), "a retired log is not empty");
        check_log(&log(1), 1).unwrap();
    }

    #[test]
    fn an_entry_is_a_closed_map_of_the_eight_fields() {
        let mut extra = entry(1);
        extra["extra"] = json!(1);
        assert!(check_log(&[extra], 0).is_err());
        for missing in ENTRY_KEYS {
            let mut e = entry(2);
            e.as_object_mut().unwrap().remove(missing);
            assert!(check_log(&[e], 0).is_err(), "missing {missing}");
        }
        assert!(check_log(&[json!([1, 2])], 0).is_err());
        assert!(check_log(&[json!(null)], 0).is_err());
    }

    #[test]
    fn an_entry_field_of_the_wrong_kind_is_refused() {
        let wrong: [(&str, Value); 9] = [
            ("index", json!("1")),
            ("index", json!(-1)),
            ("index", json!(1.5)),
            ("effective_at", json!(null)),
            ("key", json!(7)),
            ("signature", json!("")),
            ("kind", json!("revoke")),
            ("kind", json!(0)),
            ("prev", json!(false)),
        ];
        for (key, value) in wrong {
            let mut e = entry(2);
            e[key] = value.clone();
            assert!(check_log(&[e], 0).is_err(), "{key} = {value}");
        }
        let mut long = entry(2);
        long["signature"] = json!("e".repeat(MAX_ENTRY_TEXT + 1));
        assert!(check_log(&[long], 0).is_err());
        let mut null_commitment = entry(2);
        null_commitment["commitment"] = Value::Null;
        null_commitment["kind"] = json!("retire");
        check_log(&[null_commitment], 0).unwrap();
    }

    #[test]
    fn a_compromised_outcome_carries_exactly_two_logs() {
        check_compromised_logs(&[log(1), log(2)]).unwrap();
        check_compromised_logs(&[log(64), log(64)]).unwrap();
        assert!(check_compromised_logs(&[]).is_err());
        assert!(check_compromised_logs(&[log(1)]).is_err());
        assert!(check_compromised_logs(&[log(1), log(1), log(1)]).is_err());
        assert!(check_compromised_logs(&[log(1), log(0)]).is_err());
        assert!(check_compromised_logs(&[log(1), log(65)]).is_err());
    }
}
