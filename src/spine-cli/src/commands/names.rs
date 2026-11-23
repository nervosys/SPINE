//! Namespace commands use signed records; renewal keys remain on the owner's machine.
use anyhow::{ensure, Context, Result};
use clap::Subcommand;
use ed25519_dalek::SigningKey;
use spine_agent::AgentClient;
use spine_name::{
    authorise_record, log_inception_key, log_within_bounds, publication_value, resource_id,
    Current, Endpoint, NameRecord, SpineUri, SuccessionLog, SuccessionState, MAX_RECORD_TTL_SECS,
};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use tokio::net::TcpStream;

#[derive(Subcommand)]
pub enum NameAction {
    /// Inspect local identity, mesh peers, and namespace connectivity
    Status,
    /// Make a signed record and an owner key if the key path does not exist
    Create {
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        endpoint: String,
        #[arg(long, default_value = "/")]
        path: String,
        #[arg(long)]
        capability: Vec<String>,
        #[arg(long)]
        resource: Option<PathBuf>,
        /// Content type signed into the resource id; `upload` must use the same one.
        #[arg(long, default_value = "application/octet-stream")]
        content_type: String,
        /// Record lifetime in seconds, at most 604800 (7 days).
        #[arg(long, default_value_t = 3600)]
        ttl: u32,
        /// Succession log of a rotated name: `--key` is then its current key
        /// (which must already exist) and the record carries the log's head claim.
        #[arg(long)]
        log: Option<PathBuf>,
    },
    /// Rotate a name's key: append a signed `rotate` entry to its succession log.
    ///
    /// Offline. `--key` is the key that is current now (the inception key for a
    /// name with no log yet). `--reveal-key` is the key that becomes current: for
    /// the first rotation any key (created if the file does not exist); later it
    /// must be the key the previous entry committed to, as `--next-key` was then.
    /// `--next-key` is created if missing and is committed to by the new entry:
    /// keep it, it is the key of the next rotation. The log file is new; the old
    /// log is not changed. Records of the name are signed from then on with
    /// `--log` and the revealed key, and verifiers that hold the log refuse
    /// every older key.
    Rotate {
        /// The name's existing succession log, if it has rotated before.
        #[arg(long)]
        log: Option<PathBuf>,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        reveal_key: PathBuf,
        #[arg(long)]
        next_key: PathBuf,
        /// The new succession log (must not exist).
        #[arg(long)]
        output: PathBuf,
    },
    /// Retire a name for good: append a signed `retire` entry to its log.
    ///
    /// Offline. After this no key may sign records of the name, ever, for every
    /// verifier that holds the log. `--key` is the current key. `--reveal-key` is
    /// required with an existing log and must be the key the last entry
    /// committed to; with no log the inception key is revealed.
    Retire {
        #[arg(long)]
        log: Option<PathBuf>,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        reveal_key: Option<PathBuf>,
        /// The new succession log (must not exist).
        #[arg(long)]
        output: PathBuf,
    },
    /// Verify a succession log and, optionally, a record against it. Offline.
    Verify {
        log: PathBuf,
        /// A record to check: it must be authorised by the log's state.
        #[arg(long)]
        record: Option<PathBuf>,
    },
    /// Sign a tombstone that withdraws a name; publish it with `name publish`.
    ///
    /// The tombstone is for the name in RECORD, one sequence number higher, with
    /// the maximum lifetime (7 days). It supersedes every lower-seq record for
    /// the name. After it expires the name is simply unknown again, so it is not
    /// permanent revocation.
    Revoke {
        /// The name's current record: its name is revoked, at its seq plus one.
        record: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// Succession log of a rotated name: sign as its current key.
        #[arg(long)]
        log: Option<PathBuf>,
    },
    Resolve {
        name: String,
    },
    Providers {
        capability: String,
    },
    Publish {
        record: PathBuf,
        /// Succession log of a rotated name, sent with the record.
        #[arg(long)]
        log: Option<PathBuf>,
    },
    /// Re-sign and save a recoverable record before publication
    Renew {
        record: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// Record lifetime in seconds, at most 604800 (7 days).
        #[arg(long, default_value_t = 3600)]
        ttl: u32,
        /// Succession log of a rotated name: sign as its current key.
        #[arg(long)]
        log: Option<PathBuf>,
    },
    Upload {
        record: PathBuf,
        resource: PathBuf,
        #[arg(long, default_value = "application/octet-stream")]
        content_type: String,
    },
    Fetch {
        name: String,
        #[arg(long)]
        output: PathBuf,
        /// Permit loopback and private-network origins from the signed record.
        #[arg(long)]
        allow_private: bool,
    },
}

fn now() -> Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs())
}
/// Read a record, checking it. With a succession state the record is admitted
/// against it (the signer must be the current key, the head claim must match);
/// without one it is checked as before, under its inception key, and a record
/// that claims a rotation is refused because the log is missing.
fn read_record(path: &Path, state: Option<&SuccessionState>) -> Result<NameRecord> {
    let record: NameRecord = serde_json::from_slice(&std::fs::read(path)?)?;
    // Size, lifetime and the bound on `published_at` against this machine's
    // clock, then the signature.
    record.validate(now()?)?;
    match state {
        Some(state) => authorise_record(&record, Some(state))?,
        None => record.verify()?,
    }
    Ok(record)
}

/// Read and verify a succession log file: at least one entry, within the size
/// bounds, every signature and the key chain valid from its inception key.
fn read_log(path: &Path) -> Result<(SuccessionLog, SuccessionState)> {
    let log: SuccessionLog = serde_json::from_slice(&std::fs::read(path)?)
        .with_context(|| format!("{} is not a succession log", path.display()))?;
    ensure!(!log.entries.is_empty(), "the succession log has no entries");
    ensure!(
        log_within_bounds(&log),
        "the succession log is over the size bounds"
    );
    let inception = log_inception_key(&log).context("the succession log has no entries")?;
    let state = log.verify(&inception)?;
    Ok((log, state))
}

/// The optional log of a command, read.
fn read_optional_log(path: Option<&Path>) -> Result<Option<(SuccessionLog, SuccessionState)>> {
    path.map(read_log).transpose()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A key file's key, or a fresh key and its file text if the file is missing.
/// Nothing is written here: the caller writes new keys before anything that
/// depends on them.
fn key_or_new(path: &Path) -> Result<(SigningKey, Option<String>)> {
    if path.exists() {
        return Ok((read_key(path)?, None));
    }
    let seed: [u8; 32] = rand::random();
    Ok((SigningKey::from_bytes(&seed), Some(hex(&seed))))
}

/// The state of a log as the lines `rotate`, `retire` and `verify` print.
fn print_state(state: &SuccessionState) {
    println!("name: {}", SpineUri::did(*state.inception_key()));
    println!("entries: {}", state.index());
    if let Some(head) = state.head() {
        println!("head: {}", hex(head));
    }
    match state.current() {
        Current::Key(key) => println!("current key: {}", hex(&key)),
        Current::Retired => println!("retired: no key may sign this name"),
    }
    if let Some(commitment) = state.commitment() {
        println!("next key commitment: {}", hex(commitment));
    }
}

fn read_key(path: &Path) -> Result<SigningKey> {
    let value = std::fs::read_to_string(path)?;
    let value = value.trim();
    ensure!(value.len() == 64, "key must be 64 hexadecimal characters");
    let mut seed = [0; 32];
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(
            value
                .get(i * 2..i * 2 + 2)
                .context("invalid key encoding")?,
            16,
        )?;
    }
    Ok(SigningKey::from_bytes(&seed))
}

fn create_file(path: &Path, bytes: &[u8], secret: bool) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if secret { 0o600 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = secret;
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub async fn run(addr: String, action: NameAction) -> Result<()> {
    if let NameAction::Create {
        key,
        output,
        endpoint,
        path,
        capability,
        resource,
        content_type,
        ttl,
        log,
    } = action
    {
        ensure!(
            ttl > 0 && ttl <= MAX_RECORD_TTL_SECS,
            "TTL must be between 1 and {MAX_RECORD_TTL_SECS} seconds"
        );
        ensure!(!output.exists(), "output already exists");
        let logged = read_optional_log(log.as_deref())?;
        if logged.is_some() {
            // A rotated name's current key came from `rotate`; a new one here
            // would simply not be its key.
            ensure!(key.exists(), "the current key file does not exist");
        } else if !key.exists() {
            let seed: [u8; 32] = rand::random();
            create_file(&key, hex(&seed).as_bytes(), true)?;
        }
        let signing = read_key(&key)?;
        // The name is always the inception key's: after a rotation that is not
        // the key that signs.
        let authority = match &logged {
            Some((_, state)) => *state.inception_key(),
            None => signing.verifying_key().to_bytes(),
        };
        let mut record = NameRecord::new(SpineUri::did(authority).with_path(&path), 1, now()?)?
            .with_ttl(ttl)
            .with_endpoint(Endpoint::new("tcp", endpoint));
        for term in capability {
            record = record.with_capability(term);
        }
        if let Some(resource) = resource {
            record =
                record.with_content_hash(resource_id(&content_type, &std::fs::read(resource)?)?);
        }
        match &logged {
            Some((_, state)) => record.sign_current(&signing, state)?,
            None => record.sign(&signing)?,
        }
        create_file(&output, &serde_json::to_vec_pretty(&record)?, false)?;
        println!("{}", record.name);
        return Ok(());
    }
    if let NameAction::Rotate {
        log,
        key,
        reveal_key,
        next_key,
        output,
    } = action
    {
        ensure!(!output.exists(), "output already exists");
        ensure!(
            key != reveal_key && key != next_key && reveal_key != next_key,
            "--key, --reveal-key and --next-key must be three different files"
        );
        let logged = read_optional_log(log.as_deref())?;
        let (signer, fresh) = key_or_new(&key)?;
        ensure!(fresh.is_none(), "the current key file does not exist");
        let (reveal, reveal_text) = key_or_new(&reveal_key)?;
        if logged.is_some() {
            // After the first rotation the revealed key is the committed one: a
            // freshly made key could only fail the commitment.
            ensure!(
                reveal_text.is_none(),
                "the reveal key file does not exist: it is the key the last entry committed to"
            );
        }
        let (next, next_text) = key_or_new(&next_key)?;
        let (previous, inception) = match &logged {
            Some((log, state)) => (log.clone(), *state.inception_key()),
            None => (SuccessionLog::default(), signer.verifying_key().to_bytes()),
        };
        let rotated = previous.rotated(
            &inception,
            &signer,
            &reveal.verifying_key().to_bytes(),
            &next.verifying_key().to_bytes(),
            now()?,
        )?;
        let state = rotated.verify(&inception)?;
        // The keys first, the log last: a log that commits to a key nobody holds
        // would lock the name, so it is written only once the keys are on disk.
        if let Some(text) = reveal_text {
            create_file(&reveal_key, text.as_bytes(), true)?;
        }
        if let Some(text) = next_text {
            create_file(&next_key, text.as_bytes(), true)?;
        }
        create_file(&output, &serde_json::to_vec_pretty(&rotated)?, false)?;
        print_state(&state);
        println!(
            "keep: {} signs the name now; {} is committed to and must be kept for the next \
             rotation or retirement",
            reveal_key.display(),
            next_key.display()
        );
        return Ok(());
    }
    if let NameAction::Retire {
        log,
        key,
        reveal_key,
        output,
    } = action
    {
        ensure!(!output.exists(), "output already exists");
        let logged = read_optional_log(log.as_deref())?;
        let signer = read_key(&key)?;
        let (previous, inception, reveal) = match (&logged, &reveal_key) {
            (Some((log, state)), Some(path)) => (
                log.clone(),
                *state.inception_key(),
                read_key(path)?.verifying_key().to_bytes(),
            ),
            (Some(_), None) => anyhow::bail!(
                "--reveal-key is required with a log: it is the key the last entry committed to"
            ),
            (None, _) => {
                let inception = signer.verifying_key().to_bytes();
                (SuccessionLog::default(), inception, inception)
            }
        };
        let retired = previous.retired(&inception, &signer, &reveal, now()?)?;
        let state = retired.verify(&inception)?;
        create_file(&output, &serde_json::to_vec_pretty(&retired)?, false)?;
        print_state(&state);
        return Ok(());
    }
    if let NameAction::Verify { log, record } = action {
        let (_, state) = read_log(&log)?;
        print_state(&state);
        if let Some(path) = record {
            let record = read_record(&path, Some(&state))?;
            println!("record: authorised (seq {})", record.seq);
        }
        return Ok(());
    }
    if let NameAction::Revoke {
        record,
        key,
        output,
        log,
    } = action
    {
        ensure!(!output.exists(), "output already exists");
        let logged = read_optional_log(log.as_deref())?;
        let current = read_record(&record, logged.as_ref().map(|(_, state)| state))?;
        let seq = current
            .seq
            .checked_add(1)
            .context("record sequence is exhausted")?;
        let signing = read_key(&key)?;
        let mut tombstone = NameRecord::tombstone(current.name.clone(), seq, now()?)?;
        // A tombstone of a rotated name is signed by the current key and carries
        // no head claim (its shape is exact); it travels with the log.
        match &logged {
            Some((_, state)) => tombstone.sign_current(&signing, state)?,
            None => tombstone.sign(&signing)?,
        }
        create_file(&output, &serde_json::to_vec_pretty(&tombstone)?, false)?;
        println!("{}", tombstone.name);
        return Ok(());
    }
    let mut client = AgentClient::<TcpStream>::connect(&addr).await?;
    let result = match action {
        NameAction::Status => client.namespace_status().await?,
        NameAction::Resolve { name } => {
            serde_json::to_value(client.resolve_name(&name, None).await?)?
        }
        NameAction::Providers { capability } => client.find_providers(&capability, None).await?,
        NameAction::Publish { record, log } => {
            let logged = read_optional_log(log.as_deref())?;
            let record = read_record(&record, logged.as_ref().map(|(_, state)| state))?;
            let value = match &logged {
                Some((log, _)) => publication_value(&record, log)?,
                None => serde_json::to_value(record)?,
            };
            client.publish_name(value).await?
        }
        NameAction::Renew {
            record,
            key,
            output,
            ttl,
            log,
        } => {
            ensure!(
                ttl > 0 && ttl <= MAX_RECORD_TTL_SECS && !output.exists(),
                "TTL must be between 1 and {MAX_RECORD_TTL_SECS} seconds and output must not exist"
            );
            let logged = read_optional_log(log.as_deref())?;
            let current = read_record(&record, logged.as_ref().map(|(_, state)| state))?;
            let signing = read_key(&key)?;
            let renewed = match &logged {
                Some((_, state)) => current.renewed_current(&signing, state, now()?, ttl)?,
                None => current.renewed(&signing, now()?, ttl)?,
            };
            // Save before publishing so a network failure cannot lose the new
            // sequence. The saved record can be re-published idempotently.
            create_file(&output, &serde_json::to_vec_pretty(&renewed)?, false)?;
            let value = match &logged {
                Some((log, _)) => publication_value(&renewed, log)?,
                None => serde_json::to_value(renewed)?,
            };
            client.publish_name(value).await?
        }
        NameAction::Upload {
            record,
            resource,
            content_type,
        } => {
            client
                .publish_resource(
                    &read_record(&record, None)?,
                    &content_type,
                    std::fs::read(resource)?,
                )
                .await?
        }
        NameAction::Fetch {
            name,
            output,
            allow_private,
        } => {
            ensure!(!output.exists(), "output already exists");
            let policy = spine_agent::resources::FetchPolicy {
                allow_private_destinations: allow_private,
            };
            let resource = client.fetch_resource_with(&name, policy).await?;
            create_file(&output, &resource.data, false)?;
            serde_json::json!({"name":name,"bytes":resource.data.len(),"content_type":resource.content_type})
        }
        NameAction::Create { .. }
        | NameAction::Revoke { .. }
        | NameAction::Rotate { .. }
        | NameAction::Retire { .. }
        | NameAction::Verify { .. } => unreachable!(),
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
