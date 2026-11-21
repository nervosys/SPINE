//! Signed, key-pinned task RPC. Task ids are provided by the owner, never hidden retries.
use anyhow::{ensure, Result};
use clap::Subcommand;
use spine_protocol::tasks::{TaskOperation, TaskSpec};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Subcommand)]
pub enum TaskAction {
    Capabilities,
    Submit { specification: PathBuf },
    Status { id: Uuid },
    Cancel { id: Uuid },
    KnowledgePut { key: String, value: PathBuf },
    KnowledgeGet { key: String },
}

pub async fn run(addr: String, peer: String, key: String, action: TaskAction) -> Result<()> {
    ensure!(key.len() == 64, "peer key must be 64 hex characters");
    let mut pinned = [0u8; 32];
    for (index, byte) in pinned.iter_mut().enumerate() {
        let pair = key
            .get(index * 2..index * 2 + 2)
            .ok_or_else(|| anyhow::anyhow!("invalid peer key"))?;
        *byte = u8::from_str_radix(pair, 16)?;
    }
    let operation = match action {
        TaskAction::Capabilities => TaskOperation::Capabilities,
        TaskAction::Submit { specification } => {
            let spec: TaskSpec = serde_json::from_slice(&std::fs::read(specification)?)?;
            TaskOperation::Submit { spec }
        }
        TaskAction::Status { id } => TaskOperation::Status { id },
        TaskAction::Cancel { id } => TaskOperation::Cancel { id },
        TaskAction::KnowledgePut { key, value } => TaskOperation::KnowledgePut {
            key,
            value: serde_json::from_slice(&std::fs::read(value)?)?,
        },
        TaskAction::KnowledgeGet { key } => TaskOperation::KnowledgeGet { key },
    };
    let mut client = spine_agent::AgentClient::connect(&addr).await?;
    let result = client
        .mesh_call(&peer, pinned, serde_json::to_value(operation)?)
        .await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
