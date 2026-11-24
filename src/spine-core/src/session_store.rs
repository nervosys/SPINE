//! Bounded, atomic session snapshots. The manifest is authoritative after migration.
use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
};

pub const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_LEGACY_BYTES: u64 = 16 * 1024 * 1024;

/// Legacy/future timestamps receive bounded grace; expired detached records stay expired.
pub fn restored_age(retained_at: u64, now: u64, retention_secs: u64) -> Option<u64> {
    let age = if retained_at == 0 {
        0
    } else {
        now.saturating_sub(retained_at)
    };
    (age < retention_secs).then_some(age)
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn read<T: DeserializeOwned>(path: &Path, limit: u64) -> Result<T> {
    let file = std::fs::File::open(path)?;
    anyhow::ensure!(
        file.metadata()?.len() <= limit,
        "session snapshot exceeds byte limit"
    );
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= limit,
        "session snapshot exceeds byte limit"
    );
    serde_json::from_slice(&bytes).context("invalid session snapshot")
}

struct BoundedWriter<W> {
    inner: W,
    remaining: u64,
}
impl<W: Write> Write for BoundedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(std::io::Error::other("session snapshot exceeds byte limit"));
        }
        let count = self.inner.write(bytes)?;
        self.remaining -= count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub fn write<T: Serialize>(path: &Path, snapshot: &T) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(
        BoundedWriter {
            inner: temporary.as_file_mut(),
            remaining: MAX_SNAPSHOT_BYTES,
        },
        snapshot,
    )?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    // Directory durability on Unix; Windows rename/replace remains subject to filesystem guarantees.
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restore_handles_legacy_future_and_expired_timestamps() {
        assert_eq!(restored_age(0, 1000, 60), Some(0));
        assert_eq!(restored_age(2000, 1000, 60), Some(0));
        assert_eq!(restored_age(941, 1000, 60), Some(59));
        assert_eq!(restored_age(940, 1000, 60), None);
        assert_eq!(restored_age(1, 1000, 60), None);
    }
    #[test]
    fn replacement_is_authoritative_and_failed_write_preserves_previous_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions-v1.json");
        write(&path, &vec!["old", "live"]).unwrap();
        write(&path, &vec!["live"]).unwrap();
        assert_eq!(
            read::<Vec<String>>(&path, MAX_SNAPSHOT_BYTES).unwrap(),
            ["live"]
        );
        struct Refuses;
        impl Serialize for Refuses {
            fn serialize<S: serde::Serializer>(
                &self,
                _: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("fixture failure"))
            }
        }
        assert!(write(&path, &Refuses).is_err());
        assert_eq!(
            read::<Vec<String>>(&path, MAX_SNAPSHOT_BYTES).unwrap(),
            ["live"]
        );
        assert!(read::<Vec<String>>(&path, 1).is_err());
    }
    #[test]
    fn ids_and_streaming_byte_limits_are_bounded() {
        for id in ["", "../escape", "a/b", "a\\b", "a.json", "é"] {
            assert!(!valid_id(id));
        }
        assert!(valid_id("scout-node_12-abcd"));
        assert!(!valid_id(&"a".repeat(129)));
        let mut writer = BoundedWriter {
            inner: Vec::new(),
            remaining: 3,
        };
        writer.write_all(b"abc").unwrap();
        assert!(writer.write_all(b"d").is_err());
        assert_eq!(writer.inner, b"abc");
    }
}
