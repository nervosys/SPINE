//! Persistent immutable resource bytes, addressed by the hash in a signed name.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use spine_name::{content_hash, NameRecord};
use spine_protocol::resource::{ResourceChunk, MAX_RESOURCE_BYTES, MAX_RESOURCE_CHUNK};
use spine_storage::StorageBackend;
use std::sync::{Arc, Mutex};

const NAMESPACE: &str = "spine-resources-v1";
const MAX_STORED_BYTES: usize = 256 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Blob {
    content_type: String,
    data: Vec<u8>,
}

pub struct ResourceStore {
    backend: Arc<dyn StorageBackend>,
    writes: Mutex<()>,
}

impl ResourceStore {
    pub fn new(backend: Arc<dyn StorageBackend>) -> Self {
        Self {
            backend,
            writes: Mutex::new(()),
        }
    }

    pub fn put(&self, record: &NameRecord, content_type: &str, data: &[u8]) -> Result<[u8; 32]> {
        record.verify()?;
        ensure!(data.len() <= MAX_RESOURCE_BYTES, "resource exceeds 8 MiB");
        ensure!(
            !content_type.is_empty()
                && content_type.len() <= 256
                && content_type.bytes().all(|b| (32..127).contains(&b)),
            "invalid content type"
        );
        let hash = content_hash(data);
        ensure!(
            record.content_hash == Some(hash),
            "resource differs from signed content hash"
        );
        let _write = self.writes.lock().unwrap();
        if self.backend.exists(NAMESPACE, &hash)? {
            return Ok(hash);
        }
        // The bounded store makes quota computation bounded too. Quota is on
        // serialized storage bytes, including metadata and JSON overhead.
        let blob = serde_json::to_vec(&Blob {
            content_type: content_type.into(),
            data: data.into(),
        })?;
        let used: usize = self
            .backend
            .scan(NAMESPACE, None)?
            .iter()
            .map(|(_, v)| v.len())
            .sum();
        ensure!(
            used.saturating_add(blob.len()) <= MAX_STORED_BYTES,
            "resource store quota exceeded"
        );
        self.backend.put(NAMESPACE, &hash, &blob)?;
        Ok(hash)
    }

    pub fn read(&self, hash: [u8; 32], offset: u64, max_bytes: u32) -> Result<ResourceChunk> {
        ensure!(
            (1..=MAX_RESOURCE_CHUNK).contains(&max_bytes),
            "chunk must be 1 through 65536 bytes"
        );
        let stored = self
            .backend
            .get(NAMESPACE, &hash)?
            .context("resource not held by this origin")?;
        let blob: Blob = serde_json::from_slice(&stored)?;
        ensure!(
            blob.data.len() <= MAX_RESOURCE_BYTES && content_hash(&blob.data) == hash,
            "stored resource failed integrity check"
        );
        let start = usize::try_from(offset).context("invalid offset")?;
        ensure!(start <= blob.data.len(), "offset beyond resource");
        let end = start
            .saturating_add(max_bytes as usize)
            .min(blob.data.len());
        Ok(ResourceChunk {
            hash,
            offset,
            total_bytes: blob.data.len() as u64,
            content_type: blob.content_type,
            data: blob.data[start..end].to_vec(),
            complete: end == blob.data.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use spine_name::SpineUri;

    #[test]
    fn signed_resources_survive_restart_and_reject_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resources.sqlite");
        let open = || {
            ResourceStore::new(Arc::new(
                spine_storage::sqlite::SqliteBackend::open(path.to_str().unwrap()).unwrap(),
            ))
        };
        let key = SigningKey::from_bytes(&[7; 32]);
        let bytes = b"a named resource";
        let hash = content_hash(bytes);
        let mut record = NameRecord::new(SpineUri::did(key.verifying_key().to_bytes()), 1, 100)
            .unwrap()
            .with_content_hash(hash);
        record.sign(&key).unwrap();
        {
            let store = open();
            assert!(store.put(&record, "text/plain", b"wrong").is_err());
            store.put(&record, "text/plain", bytes).unwrap();
        }
        let store = open();
        let first = store.read(hash, 0, 4).unwrap();
        let second = store.read(hash, 4, 64).unwrap();
        assert!(!first.complete);
        assert!(second.complete);
        assert_eq!([first.data, second.data].concat(), bytes);
        assert!(store.read(hash, 1000, 4).is_err());
        assert!(store.read(hash, 0, MAX_RESOURCE_CHUNK + 1).is_err());
        store
            .backend
            .put(
                NAMESPACE,
                &hash,
                br#"{"content_type":"text/plain","data":[0]}"#,
            )
            .unwrap();
        assert!(store.read(hash, 0, 4).is_err());
    }
}
