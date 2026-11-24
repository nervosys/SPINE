//! Persistent immutable resource bytes, addressed by the hash in a signed name.
//!
//! Storage. Two namespaces, both keyed by the 32-byte resource id:
//!
//! - `spine-resources-v3` holds the blob as raw bytes: one version byte (`1`),
//!   the content type length as a big-endian `u32`, the content type, then the
//!   data to the end of the row.
//! - `spine-resources-meta-v3` holds a 57-byte accounting row: version byte
//!   (`1`), the 32-byte publisher key, then big-endian `u64` accounted size,
//!   stored-at time and referenced-until time.
//!
//! Verification. The id is verified when a blob is stored. A read verifies a
//! blob the first time this process serves it (a bounded set of verified ids),
//! not once per chunk; deletion and eviction drop the id from the set. The
//! client's whole-object check against the signed id is the end-to-end
//! guarantee.
//!
//! Authority. The signed record is judged once, when a blob is stored: by
//! `NameRecord::verify` (the key in the name), or by the check given to
//! `ResourceStore::with_record_authority`, which the daemon points at its
//! resolver's succession logs so that a rotated name stores under its current
//! key and a superseded key is refused (spec 02, section 10). The record is
//! not kept. A stored blob is bytes under their id and nothing else, so the
//! load and the first read check the id only, and a blob stored before its
//! name rotated or retired stays readable by id; whether a name still leads to
//! it is the resolver's answer.
//!
//! Quota. The store is bounded in bytes. Publishers share it fairly: see
//! `ResourceStore::put_at`.
use anyhow::{ensure, Context, Result};
use spine_name::{verify_resource, NameRecord, MAX_CONTENT_TYPE_LEN};
use spine_protocol::resource::{ResourceChunk, MAX_RESOURCE_BYTES, MAX_RESOURCE_CHUNK};
use spine_storage::StorageBackend;
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const NAMESPACE: &str = "spine-resources-v3";
const META_NAMESPACE: &str = "spine-resources-meta-v3";
const BLOB_VERSION: u8 = 1;
const META_VERSION: u8 = 1;
const META_LEN: usize = 1 + 32 + 8 + 8 + 8;

/// Accounted bytes for all stored resources of one node.
pub const MAX_STORED_BYTES: u64 = 256 * 1024 * 1024;
/// Bytes a publisher may hold before it counts as over its fair share: a
/// quarter of the store, as `tasks.rs` does for task entries.
pub const FAIR_SHARE_BYTES: u64 = MAX_STORED_BYTES / 4;
/// Fixed cost added to every blob's accounted size, so the number of blobs (and
/// the in-memory index) is bounded as well as the bytes.
pub const PER_BLOB_OVERHEAD: u64 = 1024;
/// Verified ids remembered per process.
const VERIFIED_CAPACITY: usize = 256;

/// The publisher recorded for blobs stored through the owner-less methods.
pub const LOCAL_OWNER: [u8; 32] = [0; 32];

/// The signing key that a record is published under, usable as the publisher
/// identity. Falls back to `LOCAL_OWNER` for a record with no key authority.
pub fn publisher_of(record: &NameRecord) -> [u8; 32] {
    record.name.public_key().copied().unwrap_or(LOCAL_OWNER)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

struct Blob {
    content_type: String,
    data: Vec<u8>,
}

fn encode_blob(content_type: &str, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + content_type.len() + data.len());
    out.push(BLOB_VERSION);
    out.extend_from_slice(&(content_type.len() as u32).to_be_bytes());
    out.extend_from_slice(content_type.as_bytes());
    out.extend_from_slice(data);
    out
}

/// An eviction candidate: stored-at time, resource id, accounted size.
type Candidate = (u64, [u8; 32], u64);

fn decode_blob(bytes: &[u8]) -> Result<Blob> {
    ensure!(bytes.len() >= 5, "stored resource is truncated");
    ensure!(
        bytes[0] == BLOB_VERSION,
        "stored resource has an unknown version"
    );
    let type_len = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
    ensure!(
        type_len <= MAX_CONTENT_TYPE_LEN && bytes.len() - 5 >= type_len,
        "stored resource has an invalid content type length"
    );
    let content_type = std::str::from_utf8(&bytes[5..5 + type_len])
        .context("stored content type is not UTF-8")?
        .to_owned();
    let data = &bytes[5 + type_len..];
    ensure!(
        data.len() <= MAX_RESOURCE_BYTES,
        "stored resource is too large"
    );
    Ok(Blob {
        content_type,
        data: data.to_vec(),
    })
}

/// What the accounting charges for a blob whose stored row is `stored_len` bytes.
fn accounted(stored_len: usize) -> u64 {
    (stored_len as u64).saturating_add(PER_BLOB_OVERHEAD)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Meta {
    owner: [u8; 32],
    /// Accounted size: stored row bytes plus `PER_BLOB_OVERHEAD`.
    size: u64,
    stored_at: u64,
    /// Latest expiry of any record that was stored with this blob. The blob is
    /// referenced (not evictable) while `now` is before it.
    ref_until: u64,
}

fn encode_meta(meta: &Meta) -> [u8; META_LEN] {
    let mut out = [0u8; META_LEN];
    out[0] = META_VERSION;
    out[1..33].copy_from_slice(&meta.owner);
    out[33..41].copy_from_slice(&meta.size.to_be_bytes());
    out[41..49].copy_from_slice(&meta.stored_at.to_be_bytes());
    out[49..57].copy_from_slice(&meta.ref_until.to_be_bytes());
    out
}

fn decode_meta(bytes: &[u8]) -> Option<Meta> {
    if bytes.len() != META_LEN || bytes[0] != META_VERSION {
        return None;
    }
    let word = |at: usize| {
        let mut w = [0u8; 8];
        w.copy_from_slice(&bytes[at..at + 8]);
        u64::from_be_bytes(w)
    };
    let mut owner = [0u8; 32];
    owner.copy_from_slice(&bytes[1..33]);
    Some(Meta {
        owner,
        size: word(33),
        stored_at: word(41),
        ref_until: word(49),
    })
}

/// In-memory copy of the accounting rows. Rebuilt and validated at startup.
#[derive(Default)]
struct Index {
    metas: HashMap<[u8; 32], Meta>,
    total: u64,
}

impl Index {
    fn held(&self, owner: &[u8; 32]) -> u64 {
        self.metas
            .values()
            .filter(|m| &m.owner == owner)
            .map(|m| m.size)
            .sum()
    }
}

/// Choose blobs to evict so that `need` accounted bytes are freed for `caller`,
/// or `None` if the policy does not allow enough to be freed. Pure: nothing is
/// deleted here.
///
/// The policy mirrors `tasks.rs` `evict_for_admission` at byte granularity:
///
/// 1. Referenced blobs are never candidates.
/// 2. A caller already holding `fair_share` bytes or more may only evict its
///    own blobs, so it can never displace another publisher.
/// 3. Otherwise owners are ranked by accounted bytes held (most first, ties by
///    the smaller key) and the victim is the oldest candidate of the first
///    owner that has one; ties on age go to the smaller id. This repeats until
///    enough is freed.
fn plan_eviction(
    index: &Index,
    caller: &[u8; 32],
    need: u64,
    referenced: &HashSet<[u8; 32]>,
    fair_share: u64,
    now: u64,
) -> Option<Vec<[u8; 32]>> {
    let mut held: BTreeMap<[u8; 32], u64> = BTreeMap::new();
    let mut queues: BTreeMap<[u8; 32], Vec<Candidate>> = BTreeMap::new();
    for (id, meta) in &index.metas {
        *held.entry(meta.owner).or_default() += meta.size;
        if meta.ref_until > now || referenced.contains(id) {
            continue;
        }
        queues
            .entry(meta.owner)
            .or_default()
            .push((meta.stored_at, *id, meta.size));
    }
    let caller_over_share = held.get(caller).copied().unwrap_or(0) >= fair_share;
    let mut queues: BTreeMap<[u8; 32], VecDeque<Candidate>> = queues
        .into_iter()
        .map(|(owner, mut entries)| {
            entries.sort_unstable();
            (owner, entries.into())
        })
        .collect();
    let mut freed = 0u64;
    let mut victims = Vec::new();
    while freed < need {
        let mut ranked: Vec<([u8; 32], u64)> = held
            .iter()
            .filter(|(owner, _)| !caller_over_share || *owner == caller)
            .map(|(owner, bytes)| (*owner, *bytes))
            .collect();
        ranked.sort_by_key(|(owner, bytes)| (Reverse(*bytes), *owner));
        let mut chosen = None;
        for (owner, _) in ranked {
            if let Some(queue) = queues.get_mut(&owner) {
                if let Some((_, id, size)) = queue.pop_front() {
                    chosen = Some((owner, id, size));
                    break;
                }
            }
        }
        let (owner, id, size) = chosen?;
        if let Some(bytes) = held.get_mut(&owner) {
            *bytes = bytes.saturating_sub(size);
        }
        freed = freed.saturating_add(size);
        victims.push(id);
    }
    Some(victims)
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

/// Why `ResourceStore::delete` failed.
#[derive(Debug)]
pub enum DeleteError {
    /// No blob is held for the hash.
    NotFound,
    /// The blob belongs to a different publisher.
    NotOwner,
    /// The backend failed.
    Internal(anyhow::Error),
}

impl std::fmt::Display for DeleteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeleteError::NotFound => f.write_str("resource not held by this origin"),
            DeleteError::NotOwner => f.write_str("resource belongs to another publisher"),
            DeleteError::Internal(error) => write!(f, "{error:#}"),
        }
    }
}

impl std::error::Error for DeleteError {}

type ReferenceSource = Box<dyn Fn() -> Result<Vec<[u8; 32]>> + Send + Sync>;

/// Judges whether a record's signer may publish for its name.
type RecordAuthorityCheck = Box<dyn Fn(&NameRecord) -> Result<()> + Send + Sync>;

/// Ids this process has hashed on read, oldest first.
#[derive(Default)]
struct Verified {
    ids: VecDeque<[u8; 32]>,
}

impl Verified {
    fn contains(&self, id: &[u8; 32]) -> bool {
        self.ids.contains(id)
    }
    fn insert(&mut self, id: [u8; 32]) {
        if self.contains(&id) {
            return;
        }
        if self.ids.len() >= VERIFIED_CAPACITY {
            self.ids.pop_front();
        }
        self.ids.push_back(id);
    }
    fn remove(&mut self, id: &[u8; 32]) {
        self.ids.retain(|v| v != id);
    }
}

/// What the store holds, read from the accounting it already keeps in memory:
/// nothing is read from the backend and no blob is hashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceUsage {
    /// Accounted bytes held by all publishers (`ResourceStore::total_bytes`).
    pub bytes: u64,
    /// Blobs held, one per accounting row.
    pub blobs: usize,
    /// The total byte limit this store admits against.
    pub limit_bytes: u64,
    /// Distinct publishers holding at least one blob.
    pub publishers: usize,
}

pub struct ResourceStore {
    backend: Arc<dyn StorageBackend>,
    /// Accounting index; also the write lock. `None` until loaded.
    index: Mutex<Option<Index>>,
    verified: Mutex<Verified>,
    /// The most recently read blob, already verified, so a chunked download
    /// does not re-read and re-parse it for every chunk.
    last: Mutex<Option<([u8; 32], Arc<Blob>)>>,
    reference_source: Option<ReferenceSource>,
    /// `None`: a record is judged by `NameRecord::verify`.
    record_authority: Option<RecordAuthorityCheck>,
    total_limit: u64,
    fair_share: u64,
    #[cfg(test)]
    read_hashes: std::sync::atomic::AtomicUsize,
}

impl ResourceStore {
    /// Open the store and validate it: a blob row whose content does not hash
    /// to its key, or that has no valid accounting row, is dropped, and so is
    /// an accounting row with no blob. If the backend cannot be read the load
    /// is retried on first use.
    pub fn new(backend: Arc<dyn StorageBackend>) -> Self {
        let store = Self {
            backend,
            index: Mutex::new(None),
            verified: Mutex::new(Verified::default()),
            last: Mutex::new(None),
            reference_source: None,
            record_authority: None,
            total_limit: MAX_STORED_BYTES,
            fair_share: FAIR_SHARE_BYTES,
            #[cfg(test)]
            read_hashes: std::sync::atomic::AtomicUsize::new(0),
        };
        if let Err(error) = store.with_index(|_| Ok(())) {
            log::warn!("resource store not loaded at startup: {error:#}");
        }
        store
    }

    /// Supply the ids that currently published name records of this node point
    /// at. Such blobs are not evictable. If the source fails, an admission that
    /// needs eviction is refused rather than evicting blindly.
    pub fn with_reference_source(
        mut self,
        source: impl Fn() -> Result<Vec<[u8; 32]>> + Send + Sync + 'static,
    ) -> Self {
        self.reference_source = Some(Box::new(source));
        self
    }

    /// Replace the check `put_at` makes on the signed record. Without this the
    /// check is `NameRecord::verify`: the key in the name signs. The daemon
    /// supplies `spine_name::LocalResolver::authorise`, which is the same for a
    /// name with no succession log and otherwise requires the name's current
    /// key, so a record signed by a superseded key, or for a retired or
    /// compromised name, stores nothing (spec 02, section 10.6).
    ///
    /// The check replaces `verify`, it is not added to it: it must itself
    /// refuse a record whose signature or lifetime is invalid. It is called
    /// once per `put_at`, before anything is read or written and outside the
    /// store's lock. It is not consulted when a blob is loaded or read.
    pub fn with_record_authority(
        mut self,
        authority: impl Fn(&NameRecord) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.record_authority = Some(Box::new(authority));
        self
    }

    /// Replace the total and per-publisher byte limits (embedders and tests).
    pub fn with_limits(mut self, total_bytes: u64, fair_share_bytes: u64) -> Self {
        self.total_limit = total_bytes;
        self.fair_share = fair_share_bytes;
        self
    }

    fn with_index<R>(&self, f: impl FnOnce(&mut Index) -> Result<R>) -> Result<R> {
        let mut guard = self.index.lock().unwrap();
        if guard.is_none() {
            *guard = Some(self.load()?);
        }
        let index = guard.as_mut().expect("index was just loaded");
        f(index)
    }

    fn load(&self) -> Result<Index> {
        let meta_rows: HashMap<Vec<u8>, Vec<u8>> = self
            .backend
            .scan(META_NAMESPACE, None)?
            .into_iter()
            .collect();
        let mut index = Index::default();
        for key in self.backend.keys(NAMESPACE)? {
            let accepted = match (
                <[u8; 32]>::try_from(key.as_slice()),
                self.backend.get(NAMESPACE, &key)?,
            ) {
                (Ok(id), Some(stored)) => match (
                    decode_blob(&stored),
                    meta_rows.get(&key).and_then(|row| decode_meta(row)),
                ) {
                    (Ok(blob), Some(meta))
                        if verify_resource(&id, &blob.content_type, &blob.data).is_ok()
                            && meta.size == accounted(stored.len()) =>
                    {
                        Some((id, meta))
                    }
                    _ => None,
                },
                _ => None,
            };
            match accepted {
                Some((id, meta)) => {
                    index.total = index.total.saturating_add(meta.size);
                    index.metas.insert(id, meta);
                }
                None => {
                    log::warn!("dropping an invalid stored resource row");
                    self.backend.delete(NAMESPACE, &key)?;
                    self.backend.delete(META_NAMESPACE, &key)?;
                }
            }
        }
        for key in meta_rows.keys() {
            let held = <[u8; 32]>::try_from(key.as_slice())
                .map(|id| index.metas.contains_key(&id))
                .unwrap_or(false);
            if !held {
                self.backend.delete(META_NAMESPACE, key)?;
            }
        }
        Ok(index)
    }

    /// Store for the local owner. Callers that know the publisher should use
    /// `put_as`, which charges the right quota share.
    pub fn put(&self, record: &NameRecord, content_type: &str, data: &[u8]) -> Result<[u8; 32]> {
        self.put_tracked(record, content_type, data)
            .map(|(hash, _created)| hash)
    }

    /// Like `put`, also reporting whether this call created the blob (false if
    /// the same content was already stored). Stores for `LOCAL_OWNER`.
    pub fn put_tracked(
        &self,
        record: &NameRecord,
        content_type: &str,
        data: &[u8],
    ) -> Result<([u8; 32], bool)> {
        self.put_as(LOCAL_OWNER, record, content_type, data)
    }

    /// Store a blob for `owner` at the current time. See `put_at`.
    pub fn put_as(
        &self,
        owner: [u8; 32],
        record: &NameRecord,
        content_type: &str,
        data: &[u8],
    ) -> Result<([u8; 32], bool)> {
        self.put_at(owner, record, content_type, data, now())
    }

    /// Store a blob for `owner`, with `now` in seconds since the Unix epoch.
    ///
    /// A record whose `published_at` is more than `MAX_CLOCK_SKEW_SECS` ahead
    /// of `now` is refused first (`NameError::PostDated`), before its signature
    /// is checked: it would extend its own life by post-dating. Then the record
    /// is verified (`NameRecord::verify`, or the check given to
    /// `with_record_authority`), the data must match the signed resource id
    /// (which commits to the content type), and the blob is charged to `owner`.
    /// Content already stored keeps its first publisher; the new record only
    /// extends the time the blob counts as referenced.
    ///
    /// Admission. The store holds at most `MAX_STORED_BYTES` of accounted
    /// bytes (stored row plus `PER_BLOB_OVERHEAD`). While there is room anyone
    /// is admitted. When there is not, unreferenced blobs are evicted under the
    /// fair-share policy of `plan_eviction`; if that cannot free enough, the
    /// publication is refused and nothing is evicted.
    pub fn put_at(
        &self,
        owner: [u8; 32],
        record: &NameRecord,
        content_type: &str,
        data: &[u8],
        now: u64,
    ) -> Result<([u8; 32], bool)> {
        record.validate_not_postdated(now)?;
        match &self.record_authority {
            Some(authority) => authority(record)?,
            None => record.verify()?,
        }
        ensure!(data.len() <= MAX_RESOURCE_BYTES, "resource exceeds 8 MiB");
        // The signed record carries the resource id, which commits to the
        // content type as well as the bytes. Invalid types and id mismatches
        // are distinct errors.
        let hash = record
            .content_hash
            .context("record carries no signed resource id")?;
        verify_resource(&hash, content_type, data)?;
        let encoded = encode_blob(content_type, data);
        let size = accounted(encoded.len());
        ensure!(size <= self.total_limit, "resource store quota exceeded");
        let until = record.expires_at();
        let created = self.with_index(|index| {
            if let Some(existing) = index.metas.get(&hash) {
                if until > existing.ref_until {
                    let mut updated = *existing;
                    updated.ref_until = until;
                    self.backend
                        .put(META_NAMESPACE, &hash, &encode_meta(&updated))?;
                    index.metas.insert(hash, updated);
                }
                return Ok(false);
            }
            let need = index
                .total
                .saturating_add(size)
                .saturating_sub(self.total_limit);
            if need > 0 {
                let mut referenced = HashSet::new();
                if let Some(source) = &self.reference_source {
                    referenced.extend(source().context("could not list referenced resources")?);
                }
                let victims = plan_eviction(index, &owner, need, &referenced, self.fair_share, now)
                    .context("resource store quota exceeded")?;
                for victim in victims {
                    self.evict_locked(index, &victim)?;
                }
            }
            let meta = Meta {
                owner,
                size,
                stored_at: now,
                ref_until: until,
            };
            // Accounting row first: a crash between the two writes leaves a
            // row with no blob, which the next load drops.
            self.backend
                .put(META_NAMESPACE, &hash, &encode_meta(&meta))?;
            if let Err(error) = self.backend.put(NAMESPACE, &hash, &encoded) {
                let _ = self.backend.delete(META_NAMESPACE, &hash);
                return Err(error);
            }
            index.total = index.total.saturating_add(size);
            index.metas.insert(hash, meta);
            Ok(true)
        })?;
        Ok((hash, created))
    }

    /// Remove a blob and its accounting. Blob row first, so a crash cannot
    /// leave a blob that no accounting covers.
    fn evict_locked(&self, index: &mut Index, hash: &[u8; 32]) -> Result<()> {
        self.backend.delete(NAMESPACE, hash)?;
        self.backend.delete(META_NAMESPACE, hash)?;
        if let Some(meta) = index.metas.remove(hash) {
            index.total = index.total.saturating_sub(meta.size);
        }
        self.verified.lock().unwrap().remove(hash);
        let mut last = self.last.lock().unwrap();
        if last.as_ref().is_some_and(|(h, _)| h == hash) {
            *last = None;
        }
        Ok(())
    }

    /// Remove a stored blob, for rolling back a publication that failed after
    /// the blob was added. Not owner-checked: it is for the daemon's own
    /// error path, not for requests. No-op if it is absent.
    pub fn remove(&self, hash: [u8; 32]) -> Result<()> {
        self.with_index(|index| self.evict_locked(index, &hash))
    }

    /// Delete a blob on behalf of `owner`, who must be the publisher recorded
    /// when it was stored (exactly that key: for a rotated name it is the
    /// inception key, `publisher_of`). Allowed even while a name still refers
    /// to it: that name then stops serving bytes, which is the owner's call.
    /// The wire's `delete_resource` calls this after checking the owner's
    /// proof (`docs/spec/00-wire.md` 12.2).
    pub fn delete(&self, owner: [u8; 32], hash: [u8; 32]) -> Result<(), DeleteError> {
        self.with_index(|index| {
            match index.metas.get(&hash) {
                None => return Ok(Err(DeleteError::NotFound)),
                Some(meta) if meta.owner != owner => return Ok(Err(DeleteError::NotOwner)),
                Some(_) => {}
            }
            self.evict_locked(index, &hash)?;
            Ok(Ok(()))
        })
        .map_err(DeleteError::Internal)?
    }

    /// Accounted bytes held by `owner`.
    pub fn held_bytes(&self, owner: &[u8; 32]) -> Result<u64> {
        self.with_index(|index| Ok(index.held(owner)))
    }

    /// Accounted bytes held by all publishers.
    pub fn total_bytes(&self) -> Result<u64> {
        self.with_index(|index| Ok(index.total))
    }

    /// The total byte limit (`MAX_STORED_BYTES` unless `with_limits` changed it).
    pub fn limit_bytes(&self) -> u64 {
        self.total_limit
    }

    /// Bytes, blobs, limit and publishers in one read of the in-memory index.
    /// The blob count is the number of accounting rows, which every put,
    /// delete and eviction already keeps in step with the bytes; the publisher
    /// count walks that map once (memory only). It fails only if the index
    /// could not be loaded, as `total_bytes` does.
    pub fn usage(&self) -> Result<ResourceUsage> {
        let limit_bytes = self.total_limit;
        self.with_index(|index| {
            let publishers: HashSet<&[u8; 32]> = index.metas.values().map(|m| &m.owner).collect();
            Ok(ResourceUsage {
                bytes: index.total,
                blobs: index.metas.len(),
                limit_bytes,
                publishers: publishers.len(),
            })
        })
    }

    fn load_verified(&self, hash: [u8; 32]) -> Result<Arc<Blob>, ReadError> {
        if let Some((cached, blob)) = self.last.lock().unwrap().as_ref() {
            if *cached == hash {
                return Ok(blob.clone());
            }
        }
        if !self.with_index(|index| Ok(index.metas.contains_key(&hash)))? {
            return Err(ReadError::NotFound);
        }
        let stored = self
            .backend
            .get(NAMESPACE, &hash)?
            .ok_or(ReadError::NotFound)?;
        let blob = decode_blob(&stored).context("stored resource failed integrity check")?;
        if !self.verified.lock().unwrap().contains(&hash) {
            #[cfg(test)]
            self.read_hashes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            verify_resource(&hash, &blob.content_type, &blob.data)
                .context("stored resource failed integrity check")?;
            self.verified.lock().unwrap().insert(hash);
        }
        let blob = Arc::new(blob);
        *self.last.lock().unwrap() = Some((hash, blob.clone()));
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
    use std::sync::atomic::Ordering;

    fn record_at(content_type: &str, bytes: &[u8], published_at: u64) -> NameRecord {
        let key = SigningKey::from_bytes(&[9; 32]);
        let mut record = NameRecord::new(
            SpineUri::did(key.verifying_key().to_bytes()),
            1,
            published_at,
        )
        .unwrap()
        .with_content_hash(resource_id(content_type, bytes).unwrap());
        record.ttl_secs = 100;
        record.sign(&key).unwrap();
        record
    }

    fn signed(bytes: &[u8]) -> NameRecord {
        record_at("text/plain", bytes, 100)
    }

    fn memory() -> ResourceStore {
        ResourceStore::new(Arc::new(spine_storage::InMemoryBackend::new()))
    }

    #[test]
    fn blob_layout_is_version_length_type_data() {
        let stored = encode_blob("text/plain", b"hi");
        let mut expected = vec![1, 0, 0, 0, 10];
        expected.extend_from_slice(b"text/plain");
        expected.extend_from_slice(b"hi");
        assert_eq!(stored, expected);
    }

    #[test]
    fn binary_round_trip_including_empty_data_and_longest_content_type() {
        let store = memory();
        let longest = "a".repeat(MAX_CONTENT_TYPE_LEN);
        for (content_type, bytes) in [
            ("text/plain", Vec::new()),
            (longest.as_str(), vec![0u8, 255, 7]),
            ("application/octet-stream", (0..=255u8).collect()),
        ] {
            let record = record_at(content_type, &bytes, 100);
            let hash = store.put(&record, content_type, &bytes).unwrap();
            let stored = store.backend.get(NAMESPACE, &hash).unwrap().unwrap();
            assert_eq!(stored.len(), 5 + content_type.len() + bytes.len());
            let chunk = store.read(hash, 0, 1024).unwrap();
            assert_eq!(chunk.content_type, content_type);
            assert_eq!(chunk.data, bytes);
            assert_eq!(chunk.total_bytes, bytes.len() as u64);
            assert!(chunk.complete);
        }
        assert!(decode_blob(&[]).is_err());
        assert!(decode_blob(&[2, 0, 0, 0, 0]).is_err());
        assert!(decode_blob(&[1, 0, 0, 0, 9, b'a']).is_err());
        assert!(decode_blob(&[1, 0, 0, 1, 1, b'a']).is_err());
    }

    #[test]
    fn signed_resources_survive_restart_and_reject_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resources.sqlite");
        let open = || {
            ResourceStore::new(Arc::new(
                spine_storage::sqlite::SqliteBackend::open(path.to_str().unwrap()).unwrap(),
            ))
        };
        let bytes = b"a named resource";
        let record = signed(bytes);
        let hash = record.content_hash.unwrap();
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
        // Corruption after a clean load is caught the first time a blob is
        // served by a process that has not verified it yet.
        let corrupted = encode_blob("text/plain", b"a named resourcX");
        let fresh = open();
        fresh.backend.put(NAMESPACE, &hash, &corrupted).unwrap();
        assert!(matches!(
            fresh.read(hash, 0, 4),
            Err(ReadError::Internal(_))
        ));
        // And the row is dropped when the store is next opened.
        let reopened = open();
        assert!(matches!(
            reopened.read(hash, 0, 4),
            Err(ReadError::NotFound)
        ));
        assert!(!reopened.backend.exists(NAMESPACE, &hash).unwrap());
        assert!(!reopened.backend.exists(META_NAMESPACE, &hash).unwrap());
        assert_eq!(reopened.total_bytes().unwrap(), 0);
    }

    #[test]
    fn a_substituted_content_type_is_rejected_on_put_and_on_read() {
        let store = memory();
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
        let loaded_before = ResourceStore::new(store.backend.clone());
        // Relabel the stored blob: right bytes, different type.
        store
            .backend
            .put(NAMESPACE, &hash, &encode_blob("text/html", bytes))
            .unwrap();
        // A store that loaded the row earlier refuses it on first read; one
        // that loads it now drops it, since it no longer hashes to its key.
        assert!(matches!(
            loaded_before.read(hash, 0, 64),
            Err(ReadError::Internal(_))
        ));
        let loaded_after = ResourceStore::new(store.backend.clone());
        assert!(matches!(
            loaded_after.read(hash, 0, 64),
            Err(ReadError::NotFound)
        ));
    }

    #[test]
    fn a_post_dated_record_is_refused_and_nothing_is_written() {
        let store = memory();
        let bytes = b"from the future";
        let now = 1000;
        let skew = spine_name::MAX_CLOCK_SKEW_SECS;
        let record = record_at("text/plain", bytes, now + skew + 1);
        let error = store
            .put_at([1; 32], &record, "text/plain", bytes, now)
            .unwrap_err();
        assert!(
            matches!(refusal(&error), spine_name::NameError::PostDated { .. }),
            "{error:#}"
        );
        assert_eq!(store.total_bytes().unwrap(), 0);
        let id = resource_id("text/plain", bytes).unwrap();
        assert!(!store.backend.exists(NAMESPACE, &id).unwrap());
        assert!(!store.backend.exists(META_NAMESPACE, &id).unwrap());
        // Exactly the allowed skew ahead is accepted.
        let edge = record_at("text/plain", bytes, now + skew);
        let (hash, created) = store
            .put_at([1; 32], &edge, "text/plain", bytes, now)
            .unwrap();
        assert!(created);
        assert_eq!(hash, id);
    }

    #[test]
    fn remove_deletes_only_the_named_blob_and_reports_creation() {
        let store = memory();
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
        assert!(!store.backend.exists(META_NAMESPACE, &hash).unwrap());
        assert_eq!(store.total_bytes().unwrap(), 0);
        store.remove(hash).unwrap();
    }

    #[test]
    fn chunked_reads_hash_the_blob_once() {
        let store = memory();
        let big = vec![7u8; 3 * 65_536 + 10];
        let other = vec![8u8; 100];
        let record = record_at("text/plain", &big, 100);
        let hash = store.put(&record, "text/plain", &big).unwrap();
        let other_record = record_at("text/plain", &other, 100);
        let other_hash = store.put(&other_record, "text/plain", &other).unwrap();
        // A store opened over the same rows has verified nothing yet.
        let store = ResourceStore::new(store.backend.clone());
        let mut offset = 0u64;
        let mut assembled = Vec::new();
        loop {
            let chunk = store.read(hash, offset, MAX_RESOURCE_CHUNK).unwrap();
            offset += chunk.data.len() as u64;
            assembled.extend_from_slice(&chunk.data);
            if chunk.complete {
                break;
            }
        }
        assert_eq!(assembled, big);
        assert_eq!(store.read_hashes.load(Ordering::SeqCst), 1);
        // Another blob displaces the one-blob cache; the first is not rehashed.
        store.read(other_hash, 0, 10).unwrap();
        assert_eq!(store.read_hashes.load(Ordering::SeqCst), 2);
        store.read(hash, 0, 10).unwrap();
        assert_eq!(store.read_hashes.load(Ordering::SeqCst), 2);
        // Deleting forgets the id.
        store.remove(hash).unwrap();
        assert!(!store.verified.lock().unwrap().contains(&hash));
    }

    /// Records for blobs of equal size, long expired at the test times.
    fn cost() -> u64 {
        accounted(5 + "text/plain".len() + 100)
    }

    fn put_n(store: &ResourceStore, owner: u8, n: u8, now: u64) -> Result<[u8; 32]> {
        let bytes = vec![n; 100];
        let record = record_at("text/plain", &bytes, 1);
        store
            .put_at([owner; 32], &record, "text/plain", &bytes, now)
            .map(|(hash, _)| hash)
    }

    fn id_of(n: u8) -> [u8; 32] {
        resource_id("text/plain", &[n; 100]).unwrap()
    }

    fn has(store: &ResourceStore, n: u8) -> bool {
        store.backend.exists(NAMESPACE, &id_of(n)).unwrap()
    }

    #[test]
    fn a_publisher_over_its_share_cannot_evict_others_but_an_under_share_one_can() {
        // Room for four blobs; a publisher is over its share at two.
        let store = memory().with_limits(4 * cost(), 2 * cost());
        put_n(&store, 2, 20, 1000).unwrap();
        for n in 1..=3 {
            put_n(&store, 1, n, 1000 + u64::from(n)).unwrap();
        }
        assert_eq!(store.total_bytes().unwrap(), 4 * cost());
        // Publisher 1 holds three blobs, over its share, and the store is
        // full: it evicts its own oldest, never publisher 2's blob.
        put_n(&store, 1, 4, 1010).unwrap();
        assert!(has(&store, 20));
        assert!(!has(&store, 1));
        assert_eq!(store.held_bytes(&[1; 32]).unwrap(), 3 * cost());
        // Publisher 2 is under its share and displaces publisher 1's oldest.
        put_n(&store, 2, 21, 1020).unwrap();
        assert!(!has(&store, 2));
        assert!(has(&store, 3) && has(&store, 4) && has(&store, 21) && has(&store, 20));
        // Publisher 2 now holds two blobs, at its share: it may only evict its own.
        put_n(&store, 2, 22, 1030).unwrap();
        assert!(!has(&store, 20));
        assert!(has(&store, 3) && has(&store, 4) && has(&store, 21) && has(&store, 22));
        assert_eq!(store.held_bytes(&[1; 32]).unwrap(), 2 * cost());
        assert_eq!(store.held_bytes(&[2; 32]).unwrap(), 2 * cost());
    }

    #[test]
    fn a_publisher_at_its_share_with_nothing_of_its_own_is_refused() {
        let store = memory().with_limits(2 * cost(), cost());
        put_n(&store, 1, 1, 1000).unwrap();
        put_n(&store, 2, 2, 1001).unwrap();
        // Publisher 1 holds exactly its share and owns only a referenced blob
        // once the reference source says so.
        let store = ResourceStore::new(store.backend.clone())
            .with_limits(2 * cost(), cost())
            .with_reference_source(|| Ok(vec![id_of(1)]));
        assert!(put_n(&store, 1, 3, 1002).is_err());
        assert!(has(&store, 1) && has(&store, 2));
    }

    #[test]
    fn referenced_blobs_are_never_evicted() {
        let source_ids = Arc::new(Mutex::new(vec![id_of(1)]));
        let shared = source_ids.clone();
        let store = memory()
            .with_limits(2 * cost(), 2 * cost())
            .with_reference_source(move || Ok(shared.lock().unwrap().clone()));
        put_n(&store, 1, 1, 1000).unwrap();
        put_n(&store, 1, 2, 1001).unwrap();
        // The oldest blob is referenced by a published name, so the next one goes.
        put_n(&store, 2, 3, 1002).unwrap();
        assert!(has(&store, 1) && !has(&store, 2) && has(&store, 3));
        // Reference both and nothing can be admitted.
        *source_ids.lock().unwrap() = vec![id_of(1), id_of(3)];
        assert!(put_n(&store, 2, 4, 1003).is_err());
        assert!(has(&store, 1) && has(&store, 3));
        // A failing source refuses rather than evicting blindly.
        let failing = ResourceStore::new(store.backend.clone())
            .with_limits(2 * cost(), 2 * cost())
            .with_reference_source(|| Err(anyhow::anyhow!("names unavailable")));
        assert!(put_n(&failing, 2, 4, 1004).is_err());
        assert!(has(&store, 1) && has(&store, 3));
    }

    #[test]
    fn a_blob_stored_with_an_unexpired_record_counts_as_referenced() {
        let store = memory().with_limits(cost(), cost());
        let bytes = vec![1u8; 100];
        // Published at 5000 with a 100 second lifetime.
        let record = record_at("text/plain", &bytes, 5000);
        store
            .put_at([1; 32], &record, "text/plain", &bytes, 5010)
            .unwrap();
        assert!(put_n(&store, 2, 2, 5020).is_err());
        assert!(has(&store, 1));
        // After the record expires the blob may be displaced.
        put_n(&store, 2, 2, 5100).unwrap();
        assert!(!has(&store, 1) && has(&store, 2));
    }

    #[test]
    fn only_the_publisher_may_delete_and_accounting_follows() {
        let store = memory();
        let hash = put_n(&store, 1, 1, 1000).unwrap();
        assert!(matches!(
            store.delete([2; 32], hash),
            Err(DeleteError::NotOwner)
        ));
        assert!(matches!(
            store.delete([1; 32], [5; 32]),
            Err(DeleteError::NotFound)
        ));
        assert!(store.read(hash, 0, 4).is_ok());
        assert_eq!(store.held_bytes(&[1; 32]).unwrap(), cost());
        assert_eq!(store.total_bytes().unwrap(), cost());
        store.delete([1; 32], hash).unwrap();
        assert!(matches!(store.read(hash, 0, 4), Err(ReadError::NotFound)));
        // Both the publisher's share and the total are released, and a second
        // delete of the same hash is `NotFound` (the daemon answers
        // `deleted: false` for it).
        assert_eq!(store.held_bytes(&[1; 32]).unwrap(), 0);
        assert_eq!(store.total_bytes().unwrap(), 0);
        assert!(matches!(
            store.delete([1; 32], hash),
            Err(DeleteError::NotFound)
        ));
        assert!(!store.backend.exists(META_NAMESPACE, &hash).unwrap());
    }

    #[test]
    fn existing_content_keeps_its_first_publisher() {
        let store = memory();
        let hash = put_n(&store, 1, 1, 1000).unwrap();
        let bytes = vec![1u8; 100];
        let record = record_at("text/plain", &bytes, 1);
        let (_, created) = store
            .put_at([2; 32], &record, "text/plain", &bytes, 1001)
            .unwrap();
        assert!(!created);
        assert_eq!(store.held_bytes(&[1; 32]).unwrap(), cost());
        assert_eq!(store.held_bytes(&[2; 32]).unwrap(), 0);
        assert!(matches!(
            store.delete([2; 32], hash),
            Err(DeleteError::NotOwner)
        ));
    }

    /// The blob count and the byte total follow every way a blob comes and goes,
    /// and agree with what the backend holds.
    #[test]
    fn usage_follows_put_delete_eviction_and_reopen() {
        let backend = Arc::new(spine_storage::InMemoryBackend::new());
        let store = ResourceStore::new(backend.clone()).with_limits(4 * cost(), 2 * cost());
        let rows = |store: &ResourceStore| {
            let usage = store.usage().unwrap();
            assert_eq!(
                usage.blobs,
                store.backend.scan(NAMESPACE, None).unwrap().len(),
                "one accounting row per stored blob"
            );
            assert_eq!(
                usage.blobs,
                store.backend.scan(META_NAMESPACE, None).unwrap().len()
            );
            assert_eq!(usage.bytes, store.total_bytes().unwrap());
            usage
        };
        let empty = rows(&store);
        assert_eq!(
            (
                empty.bytes,
                empty.blobs,
                empty.limit_bytes,
                empty.publishers
            ),
            (0, 0, 4 * cost(), 0)
        );

        // Puts, from two publishers; the same content again adds nothing.
        put_n(&store, 2, 20, 1000).unwrap();
        for n in 1..=3 {
            put_n(&store, 1, n, 1000 + u64::from(n)).unwrap();
        }
        put_n(&store, 1, 1, 1005).unwrap();
        let full = rows(&store);
        assert_eq!((full.blobs, full.publishers), (4, 2));
        assert_eq!(full.bytes, 4 * cost());

        // An eviction frees one blob and the admission adds one.
        put_n(&store, 1, 4, 1010).unwrap();
        assert_eq!(rows(&store).blobs, 4);
        // An under-share publisher displaces another's blob.
        put_n(&store, 2, 21, 1020).unwrap();
        assert_eq!(rows(&store).blobs, 4);

        // Deletes.
        store.delete([2; 32], id_of(20)).unwrap();
        let after = rows(&store);
        assert_eq!(after.blobs, 3);
        assert_eq!(after.bytes, 3 * cost());
        assert!(store.delete([2; 32], [5; 32]).is_err());
        assert_eq!(rows(&store), after, "a refused delete changes nothing");

        // A reopen rebuilds the same counts from the rows.
        let reopened = ResourceStore::new(backend).with_limits(4 * cost(), 2 * cost());
        assert_eq!(rows(&reopened), after);
    }

    #[test]
    fn accounting_survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resources.sqlite");
        let open = || {
            ResourceStore::new(Arc::new(
                spine_storage::sqlite::SqliteBackend::open(path.to_str().unwrap()).unwrap(),
            ))
            .with_limits(3 * cost(), 2 * cost())
        };
        let (a1, a2) = {
            let store = open();
            let a1 = put_n(&store, 1, 1, 1000).unwrap();
            let a2 = put_n(&store, 1, 2, 1001).unwrap();
            put_n(&store, 2, 3, 1002).unwrap();
            (a1, a2)
        };
        let store = open();
        assert_eq!(store.held_bytes(&[1; 32]).unwrap(), 2 * cost());
        assert_eq!(store.held_bytes(&[2; 32]).unwrap(), cost());
        assert_eq!(store.total_bytes().unwrap(), 3 * cost());
        assert!(matches!(
            store.delete([2; 32], a1),
            Err(DeleteError::NotOwner)
        ));
        // Publisher 1 is at its share, so it evicts its own oldest (a1), and
        // the stored time survived the reopen to make that choice.
        put_n(&store, 1, 4, 1003).unwrap();
        assert!(!has(&store, 1));
        assert!(store.read(a2, 0, 4).is_ok());
        assert!(has(&store, 3));
        store.delete([1; 32], a2).unwrap();
        assert_eq!(store.held_bytes(&[1; 32]).unwrap(), cost());
    }

    #[test]
    fn load_drops_rows_without_valid_accounting_and_accounting_without_rows() {
        let store = memory();
        let kept = put_n(&store, 1, 1, 1000).unwrap();
        let unaccounted = put_n(&store, 1, 2, 1001).unwrap();
        let orphan_meta = put_n(&store, 1, 3, 1002).unwrap();
        let badsize = put_n(&store, 1, 4, 1003).unwrap();
        store.backend.delete(META_NAMESPACE, &unaccounted).unwrap();
        store.backend.delete(NAMESPACE, &orphan_meta).unwrap();
        store
            .backend
            .put(META_NAMESPACE, &badsize, &[1, 2, 3])
            .unwrap();
        let reopened = ResourceStore::new(store.backend.clone());
        assert_eq!(reopened.total_bytes().unwrap(), cost());
        assert!(reopened.read(kept, 0, 4).is_ok());
        for gone in [unaccounted, orphan_meta, badsize] {
            assert!(matches!(
                reopened.read(gone, 0, 4),
                Err(ReadError::NotFound)
            ));
            assert!(!reopened.backend.exists(NAMESPACE, &gone).unwrap());
            assert!(!reopened.backend.exists(META_NAMESPACE, &gone).unwrap());
        }
    }

    #[test]
    fn a_blob_larger_than_the_whole_store_is_refused() {
        let store = memory().with_limits(cost() - 1, cost() - 1);
        assert!(put_n(&store, 1, 1, 1000).is_err());
        assert_eq!(store.total_bytes().unwrap(), 0);
    }

    // ---- record authority and key succession (spec 02, section 10) ----

    fn key(n: u8) -> SigningKey {
        SigningKey::from_bytes(&[n; 32])
    }

    fn public(key: &SigningKey) -> [u8; 32] {
        key.verifying_key().to_bytes()
    }

    /// An unsigned binding of `bytes` to the name whose inception key is
    /// `name_key`.
    fn binding(name_key: &SigningKey, bytes: &[u8], seq: u64) -> NameRecord {
        let mut record = NameRecord::new(SpineUri::did(public(name_key)), seq, 100)
            .unwrap()
            .with_content_hash(resource_id("text/plain", bytes).unwrap());
        record.ttl_secs = 100;
        record
    }

    /// The name of `key(20)`, rotated once: `key(21)` is current and the entry
    /// commits to `key(22)`.
    fn rotation() -> spine_name::SuccessionLog {
        spine_name::SuccessionLog::default()
            .rotated(
                &public(&key(20)),
                &key(20),
                &public(&key(21)),
                &public(&key(22)),
                90,
            )
            .unwrap()
    }

    /// A binding for the rotated name signed by its current key, with the
    /// head claim.
    fn by_current_key(bytes: &[u8], seq: u64) -> NameRecord {
        let state = rotation().verify(&public(&key(20))).unwrap();
        let mut record = binding(&key(20), bytes, seq);
        record.sign_current(&key(21), &state).unwrap();
        record
    }

    /// A binding signed by the key in the name: the only signer of a name
    /// without a log, and a superseded key once the name has rotated.
    fn by_inception_key(name_key: &SigningKey, bytes: &[u8], seq: u64) -> NameRecord {
        let mut record = binding(name_key, bytes, seq);
        record.sign(name_key).unwrap();
        record
    }

    /// A store over `backend` whose records are judged by `names`, as the
    /// daemon wires it.
    fn judged_by(
        backend: Arc<dyn StorageBackend>,
        names: &Arc<spine_name::LocalResolver>,
    ) -> ResourceStore {
        let names = names.clone();
        ResourceStore::new(backend).with_record_authority(move |record: &NameRecord| {
            names.authorise(record).map_err(anyhow::Error::from)
        })
    }

    fn refusal(error: &anyhow::Error) -> &spine_name::NameError {
        error
            .downcast_ref::<spine_name::NameError>()
            .unwrap_or_else(|| panic!("not a name error: {error:#}"))
    }

    #[test]
    fn without_a_record_authority_the_key_in_the_name_signs_as_before() {
        // No hook: the store knows nothing of succession. The inception key is
        // accepted and the current key of a rotated name is a bad signature.
        let store = memory();
        let old = by_inception_key(&key(20), b"old key", 1);
        let hash = store.put(&old, "text/plain", b"old key").unwrap();
        assert_eq!(store.read(hash, 0, 64).unwrap().data, b"old key");
        let fresh = by_current_key(b"current key", 2);
        let error = store.put(&fresh, "text/plain", b"current key").unwrap_err();
        assert_eq!(refusal(&error), &spine_name::NameError::BadSignature);
        assert!(matches!(
            store.read(fresh.content_hash.unwrap(), 0, 64),
            Err(ReadError::NotFound)
        ));
    }

    #[test]
    fn a_rotated_name_stores_under_its_current_key_and_not_a_superseded_one() {
        let names = Arc::new(spine_name::LocalResolver::at_time(100));
        let store = judged_by(Arc::new(spine_storage::InMemoryBackend::new()), &names);
        let fresh = by_current_key(b"current key", 2);

        // The claim cannot be judged until the log is held.
        let error = store.put(&fresh, "text/plain", b"current key").unwrap_err();
        assert!(
            matches!(
                refusal(&error),
                spine_name::NameError::NeedSuccessionLog { .. }
            ),
            "{error:#}"
        );
        assert_eq!(store.total_bytes().unwrap(), 0);

        names.offer_log(rotation()).unwrap();
        let (hash, created) = store
            .put_as(publisher_of(&fresh), &fresh, "text/plain", b"current key")
            .unwrap();
        assert!(created);
        assert_eq!(store.read(hash, 0, 64).unwrap().data, b"current key");
        // The quota share is the name's (its inception key), whichever key signs.
        assert_eq!(publisher_of(&fresh), public(&key(20)));
        assert_eq!(store.held_bytes(&public(&key(20))).unwrap(), cost_of(11));

        // The superseded key stores nothing, for new content or for content
        // already held.
        let stale = by_inception_key(&key(20), b"old key", 9);
        let error = store.put(&stale, "text/plain", b"old key").unwrap_err();
        assert!(
            matches!(refusal(&error), spine_name::NameError::SupersededKey(_)),
            "{error:#}"
        );
        assert!(matches!(
            store.read(stale.content_hash.unwrap(), 0, 64),
            Err(ReadError::NotFound)
        ));
        let stale_same = by_inception_key(&key(20), b"current key", 9);
        let error = store
            .put(&stale_same, "text/plain", b"current key")
            .unwrap_err();
        assert!(
            matches!(refusal(&error), spine_name::NameError::SupersededKey(_)),
            "{error:#}"
        );
        assert_eq!(store.total_bytes().unwrap(), cost_of(11));
    }

    /// Accounted size of a `text/plain` blob of `len` bytes.
    fn cost_of(len: usize) -> u64 {
        accounted(5 + "text/plain".len() + len)
    }

    #[test]
    fn a_retired_name_stores_nothing() {
        let names = Arc::new(spine_name::LocalResolver::at_time(100));
        let store = judged_by(Arc::new(spine_storage::InMemoryBackend::new()), &names);
        // Retired at once: the inception key signs entry 1 and reveals itself.
        let retired = spine_name::SuccessionLog::default()
            .retired(&public(&key(20)), &key(20), &public(&key(20)), 90)
            .unwrap();
        names.offer_log(retired).unwrap();
        let record = by_inception_key(&key(20), b"too late", 1);
        let error = store.put(&record, "text/plain", b"too late").unwrap_err();
        assert!(
            matches!(refusal(&error), spine_name::NameError::Retired(_)),
            "{error:#}"
        );
        assert_eq!(store.total_bytes().unwrap(), 0);
    }

    #[test]
    fn a_name_without_a_log_is_judged_exactly_as_before() {
        // Both an empty book and a book that holds another name's log.
        for other_log in [false, true] {
            let names = Arc::new(spine_name::LocalResolver::at_time(100));
            if other_log {
                names.offer_log(rotation()).unwrap();
            }
            let store = judged_by(Arc::new(spine_storage::InMemoryBackend::new()), &names);
            let plain = memory();

            let record = signed(b"unrotated");
            let hash = store.put(&record, "text/plain", b"unrotated").unwrap();
            assert_eq!(
                hash,
                plain.put(&record, "text/plain", b"unrotated").unwrap()
            );
            assert_eq!(store.read(hash, 0, 64).unwrap().data, b"unrotated");

            // Every refusal is the one the store without a hook gives.
            let mut forged = signed(b"forged");
            forged.seq += 1;
            let mismatched = signed(b"signed bytes");
            for (record, data) in [(&forged, &b"forged"[..]), (&mismatched, &b"other"[..])] {
                let judged = store.put(record, "text/plain", data).unwrap_err();
                let bare = plain.put(record, "text/plain", data).unwrap_err();
                assert_eq!(refusal(&judged), refusal(&bare));
            }
            assert_eq!(
                refusal(&store.put(&forged, "text/plain", b"forged").unwrap_err()),
                &spine_name::NameError::BadSignature
            );
        }
    }

    #[test]
    fn a_blob_stored_before_a_rotation_stays_readable_by_id() {
        let backend: Arc<dyn StorageBackend> = Arc::new(spine_storage::InMemoryBackend::new());
        let names = Arc::new(spine_name::LocalResolver::at_time(100));
        let store = judged_by(backend.clone(), &names);
        let old = by_inception_key(&key(20), b"kept bytes", 1);
        let hash = store.put(&old, "text/plain", b"kept bytes").unwrap();

        // The name rotates. The record that brought the blob is no longer
        // authorised, and cannot be used to store again.
        names.offer_log(rotation()).unwrap();
        let error = store.put(&old, "text/plain", b"kept bytes").unwrap_err();
        assert!(
            matches!(refusal(&error), spine_name::NameError::SupersededKey(_)),
            "{error:#}"
        );
        // The bytes are still the bytes of their id: served now, and kept by
        // a store that loads them afterwards.
        assert_eq!(store.read(hash, 0, 64).unwrap().data, b"kept bytes");
        let reopened = judged_by(backend, &names);
        assert_eq!(reopened.read(hash, 0, 64).unwrap().data, b"kept bytes");
        assert_eq!(reopened.total_bytes().unwrap(), cost_of(10));

        // The current key binds the same bytes again without a second copy.
        let fresh = by_current_key(b"kept bytes", 2);
        assert_eq!(fresh.content_hash, Some(hash));
        let (again, created) = reopened
            .put_tracked(&fresh, "text/plain", b"kept bytes")
            .unwrap();
        assert_eq!(again, hash);
        assert!(!created);
    }
}
