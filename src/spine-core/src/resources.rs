//! Persistent immutable resource bytes, addressed by the hash in a signed name.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use spine_name::{verify_resource, NameRecord};
use spine_protocol::resource::{ResourceChunk, MAX_RESOURCE_BYTES, MAX_RESOURCE_CHUNK};
use spine_storage::StorageBackend;
use std::sync::{Arc, Mutex};

const NAMESPACE: &str = "spine-resources-v2";
const MAX_STORED_BYTES: usize = 256 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Blob {
    content_type: String,
    #[serde(with = "compact_bytes")]
    data: Vec<u8>,
}

/// Bytes are stored as a base64 string (about 1.33 bytes per data byte). Stores
/// written before this encoding held a JSON integer array; loading accepts both.
mod compact_bytes {
    use base64::Engine;
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(data))
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
        Text(String),
        Legacy(Vec<u8>),
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        match Repr::deserialize(deserializer)? {
            Repr::Text(text) => base64::engine::general_purpose::STANDARD
                .decode(text)
                .map_err(D::Error::custom),
            Repr::Legacy(bytes) => Ok(bytes),
        }
    }
}

/// Why `ResourceStore::read` failed. The daemon maps the variant to a wire code;
/// nothing inspects the message.
#[derive(Debug)]
pub enum ReadError {
    /// No blob is held for the hash.
    NotFound,
    /// `max_bytes` outside 1 to 65,536, or an `offset` beyond the resource.
    InvalidRange(&'static str),
    /// The stored blob could not be read or failed its integrity check.
    Internal(anyhow::Error),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::NotFound => f.write_str("resource not held by this origin"),
            ReadError::InvalidRange(reason) => f.write_str(reason),
            ReadError::Internal(error) => write!(f, "{error:#}"),
        }
    }
}

impl std::error::Error for ReadError {}

impl From<anyhow::Error> for ReadError {
    fn from(error: anyhow::Error) -> Self {
        ReadError::Internal(error)
    }
}

pub struct ResourceStore {
    backend: Arc<dyn StorageBackend>,
    writes: Mutex<()>,
    /// The most recently read blob, already checked against its hash, so a
    /// chunked download parses and hashes it once rather than per chunk.
    verified: Mutex<Option<([u8; 32], Arc<Blob>)>>,
}

impl ResourceStore {
    pub fn new(backend: Arc<dyn StorageBackend>) -> Self {
        Self {
            backend,
            writes: Mutex::new(()),
            verified: Mutex::new(None),
        }
    }

    pub fn put(&self, record: &NameRecord, content_type: &str, data: &[u8]) -> Result<[u8; 32]> {
        self.put_tracked(record, content_type, data)
            .map(|(hash, _created)| hash)
    }

    /// Like `put`, also reporting whether this call created the blob (false if
    /// the same content was already stored).
    pub fn put_tracked(
        &self,
        record: &NameRecord,
        content_type: &str,
        data: &[u8],
    ) -> Result<([u8; 32], bool)> {
        record.verify()?;
        ensure!(data.len() <= MAX_RESOURCE_BYTES, "resource exceeds 8 MiB");
        // The signed record carries the resource id, which commits to the
        // content type as well as the bytes. Invalid types and id mismatches
        // are distinct errors.
        let hash = record
            .content_hash
            .context("record carries no signed resource id")?;
        verify_resource(&hash, content_type, data)?;
        let _write = self.writes.lock().unwrap();
        if self.backend.exists(NAMESPACE, &hash)? {
            return Ok((hash, false));
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
        Ok((hash, true))
    }

    /// Remove a stored blob, for rolling back a publication that failed after
    /// the blob was added. No-op if it is absent.
    pub fn remove(&self, hash: [u8; 32]) -> Result<()> {
        let _write = self.writes.lock().unwrap();
        self.backend.delete(NAMESPACE, &hash)?;
        let mut cache = self.verified.lock().unwrap();
        if cache.as_ref().is_some_and(|(h, _)| *h == hash) {
            *cache = None;
        }
        Ok(())
    }

    fn load_verified(&self, hash: [u8; 32]) -> Result<Arc<Blob>, ReadError> {
        if let Some((cached, blob)) = self.verified.lock().unwrap().as_ref() {
            if *cached == hash {
                return Ok(blob.clone());
            }
        }
        let stored = self
            .backend
            .get(NAMESPACE, &hash)?
            .ok_or(ReadError::NotFound)?;
        let blob: Blob = serde_json::from_slice(&stored).map_err(anyhow::Error::from)?;
        if blob.data.len() > MAX_RESOURCE_BYTES {
            return Err(ReadError::Internal(anyhow::anyhow!(
                "stored resource failed integrity check"
            )));
        }
        verify_resource(&hash, &blob.content_type, &blob.data)
            .context("stored resource failed integrity check")?;
        let blob = Arc::new(blob);
        *self.verified.lock().unwrap() = Some((hash, blob.clone()));
        Ok(blob)
    }

    pub fn read(
        &self,
        hash: [u8; 32],
        offset: u64,
        max_bytes: u32,
    ) -> Result<ResourceChunk, ReadError> {
        if !(1..=MAX_RESOURCE_CHUNK).contains(&max_bytes) {
            return Err(ReadError::InvalidRange(
                "chunk must be 1 through 65536 bytes",
            ));
        }
        let blob = self.load_verified(hash)?;
        let start = usize::try_from(offset)
            .ok()
            .filter(|start| *start <= blob.data.len())
            .ok_or(ReadError::InvalidRange("offset beyond resource"))?;
        let end = start
            .saturating_add(max_bytes as usize)
            .min(blob.data.len());
        Ok(ResourceChunk {
            hash,
            offset,
            total_bytes: blob.data.len() as u64,
            content_type: blob.content_type.clone(),
            data: blob.data[start..end].to_vec(),
            complete: end == blob.data.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use spine_name::{resource_id, SpineUri};

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
        let hash = resource_id("text/plain", bytes).unwrap();
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
        {
            let store = open();
            let first = store.read(hash, 0, 4).unwrap();
            assert!(!first.complete);
        }
        let first = store.read(hash, 0, 4).unwrap();
        let second = store.read(hash, 4, 64).unwrap();
        assert!(!first.complete);
        assert!(second.complete);
        assert_eq!([first.data, second.data].concat(), bytes);
        assert!(matches!(
            store.read(hash, 1000, 4),
            Err(ReadError::InvalidRange(_))
        ));
        assert!(matches!(
            store.read(hash, 0, MAX_RESOURCE_CHUNK + 1),
            Err(ReadError::InvalidRange(_))
        ));
        assert!(matches!(
            store.read([1; 32], 0, 4),
            Err(ReadError::NotFound)
        ));
        // Corruption is caught the first time a blob is loaded.
        let store = open();
        store
            .backend
            .put(
                NAMESPACE,
                &hash,
                br#"{"content_type":"text/plain","data":[0]}"#,
            )
            .unwrap();
        assert!(matches!(
            store.read(hash, 0, 4),
            Err(ReadError::Internal(_))
        ));
    }

    fn signed(bytes: &[u8]) -> NameRecord {
        let key = SigningKey::from_bytes(&[9; 32]);
        let mut record = NameRecord::new(SpineUri::did(key.verifying_key().to_bytes()), 1, 100)
            .unwrap()
            .with_content_hash(resource_id("text/plain", bytes).unwrap());
        record.sign(&key).unwrap();
        record
    }

    #[test]
    fn a_substituted_content_type_is_rejected_on_put_and_on_read() {
        let store = ResourceStore::new(Arc::new(spine_storage::InMemoryBackend::new()));
        let bytes = b"typed bytes";
        let record = signed(bytes);
        let err = store.put(&record, "text/html", bytes).unwrap_err();
        assert_eq!(
            err.downcast_ref::<spine_name::NameError>(),
            Some(&spine_name::NameError::ResourceIdMismatch)
        );
        let err = store.put(&record, "Text/Plain", bytes).unwrap_err();
        assert_eq!(
            err.downcast_ref::<spine_name::NameError>(),
            Some(&spine_name::NameError::InvalidContentType)
        );
        let hash = store.put(&record, "text/plain", bytes).unwrap();
        assert_eq!(store.read(hash, 0, 64).unwrap().content_type, "text/plain");
        // Relabel the stored blob: right bytes, different type.
        let relabelled = serde_json::to_vec(&serde_json::json!({
            "content_type": "text/html",
            "data": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
        }))
        .unwrap();
        store.backend.put(NAMESPACE, &hash, &relabelled).unwrap();
        let fresh = ResourceStore::new(store.backend.clone());
        assert!(fresh.read(hash, 0, 64).is_err());
    }

    #[test]
    fn remove_deletes_only_the_named_blob_and_reports_creation() {
        let store = ResourceStore::new(Arc::new(spine_storage::InMemoryBackend::new()));
        let bytes = b"rolled back";
        let record = signed(bytes);
        let (hash, created) = store.put_tracked(&record, "text/plain", bytes).unwrap();
        assert!(created);
        let (_, again) = store.put_tracked(&record, "text/plain", bytes).unwrap();
        assert!(!again);
        assert!(store.read(hash, 0, 4).is_ok());
        store.remove(hash).unwrap();
        assert!(matches!(store.read(hash, 0, 4), Err(ReadError::NotFound)));
        assert!(!store.backend.exists(NAMESPACE, &hash).unwrap());
        store.remove(hash).unwrap();
    }

    #[test]
    fn stores_compactly_and_still_reads_the_legacy_integer_array() {
        let store = ResourceStore::new(Arc::new(spine_storage::InMemoryBackend::new()));
        let bytes = [200u8; 1000];
        let record = signed(&bytes);
        let hash = store.put(&record, "text/plain", &bytes).unwrap();
        let stored = store.backend.get(NAMESPACE, &hash).unwrap().unwrap();
        assert!(stored.len() < 1500, "stored {} bytes", stored.len());
        let legacy = serde_json::to_vec(&serde_json::json!({
            "content_type": "text/plain",
            "data": bytes.to_vec(),
        }))
        .unwrap();
        store.backend.put(NAMESPACE, &hash, &legacy).unwrap();
        let fresh = ResourceStore::new(store.backend.clone());
        assert_eq!(fresh.read(hash, 0, 2000).unwrap().data, bytes);
    }
}
