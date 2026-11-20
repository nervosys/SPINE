//! SPINE Core — library interface
//!
//! Provides `SpineConfig` and `serve()` for embedding the SPINE server
//! in other binaries (e.g. `spine-cli deploy`).

pub mod config;
pub mod connections;
pub mod operations;
pub mod outbound;
pub mod resources;
pub mod session_store;
pub mod tasks;
pub mod tls;
pub mod tools;

pub use config::SpineConfig;

/// Start the SPINE server with the given configuration.
///
/// This is equivalent to running the `spine-core` binary.
/// The function blocks until the server shuts down.
pub async fn serve(config: SpineConfig) -> anyhow::Result<()> {
    config.validate()?;
    use std::io::Write;
    let mut snapshot = tempfile::NamedTempFile::new()?;
    snapshot.write_all(toml::to_string(&config)?.as_bytes())?;
    snapshot.as_file().sync_all()?;
    let snapshot = snapshot.into_temp_path();

    // Find the spine-core binary and spawn it
    let exe = std::env::current_exe()?;
    let exe_dir = exe.parent().unwrap_or(std::path::Path::new("."));

    // Look for spine-core binary nearby
    let core_bin = if cfg!(windows) {
        exe_dir.join("spine-core.exe")
    } else {
        exe_dir.join("spine-core")
    };

    if core_bin.exists() {
        // Spawn the spine-core binary
        let mut command = tokio::process::Command::new(&core_bin);
        command
            .kill_on_drop(true)
            .env("SPINE_CONFIG_PATH", snapshot.as_os_str())
            .env("SPINE_MANAGED_CHILD", "1")
            .stdin(std::process::Stdio::piped());
        // The supplied configuration is authoritative for this child. Leave
        // unrelated model/service environment alone and never mutate the parent.
        for key in [
            "PORT",
            "SPINE_PORT",
            "SPINE_HOST",
            "SPINE_METRICS_PORT",
            "SPINE_MAX_SESSIONS",
            "SPINE_TLS",
            "SPINE_TLS_CERT",
            "SPINE_TLS_KEY",
            "SPINE_TLS_CA",
            "SPINE_TLS_MTLS",
            "SPINE_TLS_CRL",
            "SPINE_TLS_AUTO_GENERATE",
            "SPINE_LOG_FORMAT",
            "SPINE_LOG_LEVEL",
            "SPINE_REGION",
            "SPINE_IDLE_TIMEOUT",
            "SPINE_SHUTDOWN_TIMEOUT",
            "SPINE_SEEDS",
            "SPINE_ADVERTISE",
        ] {
            command.env_remove(key);
        }
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn()?;
        let mut control = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("managed child control pipe is missing"))?;
        let status = tokio::select! {
            status=child.wait()=>status?,
            signal=tokio::signal::ctrl_c()=> {
                signal?;
                use tokio::io::AsyncWriteExt;
                let _=control.write_all(&[1]).await;
                drop(control);
                match tokio::time::timeout(std::time::Duration::from_secs(config.server.shutdown_timeout_secs.saturating_add(5)),child.wait()).await {
                    Ok(status)=>status?,
                    Err(_)=>{child.kill().await?;anyhow::bail!("managed child exceeded its shutdown deadline");}
                }
            }
        };
        if !status.success() {
            anyhow::bail!("spine-core exited with status: {}", status);
        }
    } else {
        // Inline server start using the main.rs logic
        // This is a fallback — in production, use the binary
        eprintln!("spine-core binary not found at {:?}", core_bin);
        eprintln!("Build it first: cargo build -p spine-core");
        eprintln!("Or run directly: cargo run -p spine-core");
        anyhow::bail!("spine-core binary not found. Run `cargo build -p spine-core` first.");
    }

    Ok(())
}
