//! Namespace commands use signed records; renewal keys remain on the owner's machine.
use anyhow::{ensure, Context, Result};
use clap::Subcommand;
use ed25519_dalek::SigningKey;
use spine_agent::AgentClient;
use spine_name::{resource_id, Endpoint, NameRecord, SpineUri};
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
        #[arg(long, default_value_t = 3600)]
        ttl: u32,
    },
    Resolve {
        name: String,
    },
    Providers {
        capability: String,
    },
    Publish {
        record: PathBuf,
    },
    /// Re-sign and save a recoverable record before publication
    Renew {
        record: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 3600)]
        ttl: u32,
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
fn read_record(path: &Path) -> Result<NameRecord> {
    let record: NameRecord = serde_json::from_slice(&std::fs::read(path)?)?;
    record.verify()?;
    Ok(record)
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
    } = action
    {
        ensure!(ttl > 0, "TTL must be positive");
        ensure!(!output.exists(), "output already exists");
        if !key.exists() {
            let seed: [u8; 32] = rand::random();
            let text: String = seed.iter().map(|b| format!("{b:02x}")).collect();
            create_file(&key, text.as_bytes(), true)?;
        }
        let signing = read_key(&key)?;
        let mut record = NameRecord::new(
            SpineUri::did(signing.verifying_key().to_bytes()).with_path(&path),
            1,
            now()?,
        )?
        .with_ttl(ttl)
        .with_endpoint(Endpoint::new("tcp", endpoint));
        for term in capability {
            record = record.with_capability(term);
        }
        if let Some(resource) = resource {
            record =
                record.with_content_hash(resource_id(&content_type, &std::fs::read(resource)?)?);
        }
        record.sign(&signing)?;
        create_file(&output, &serde_json::to_vec_pretty(&record)?, false)?;
        println!("{}", record.name);
        return Ok(());
    }
    let mut client = AgentClient::<TcpStream>::connect(&addr).await?;
    let result = match action {
        NameAction::Status => client.namespace_status().await?,
        NameAction::Resolve { name } => {
            serde_json::to_value(client.resolve_name(&name, None).await?)?
        }
        NameAction::Providers { capability } => client.find_providers(&capability, None).await?,
        NameAction::Publish { record } => {
            client
                .publish_name(serde_json::to_value(read_record(&record)?)?)
                .await?
        }
        NameAction::Renew {
            record,
            key,
            output,
            ttl,
        } => {
            ensure!(
                ttl > 0 && !output.exists(),
                "TTL must be positive and output must not exist"
            );
            let renewed = read_record(&record)?.renewed(&read_key(&key)?, now()?, ttl)?;
            // Save before publishing so a network failure cannot lose the new
            // sequence. The saved record can be re-published idempotently.
            create_file(&output, &serde_json::to_vec_pretty(&renewed)?, false)?;
            client.publish_name(serde_json::to_value(renewed)?).await?
        }
        NameAction::Upload {
            record,
            resource,
            content_type,
        } => {
            client
                .publish_resource(
                    &read_record(&record)?,
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
        NameAction::Create { .. } => unreachable!(),
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
