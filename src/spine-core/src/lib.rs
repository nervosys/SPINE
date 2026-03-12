//! SPINE Core — library interface
//!
//! Provides `SpineConfig` and `serve()` for embedding the SPINE server
//! in other binaries (e.g. `spine-cli deploy`).

pub mod config;
pub mod tls;

pub use config::SpineConfig;

/// Start the SPINE server with the given configuration.
///
/// This is equivalent to running the `spine-core` binary.
/// The function blocks until the server shuts down.
pub async fn serve(config: SpineConfig) -> anyhow::Result<()> {
    // Set env vars so the config is picked up by the spawned process
    std::env::set_var("SPINE_HOST", &config.server.host);
    std::env::set_var("SPINE_PORT", config.server.port.to_string());
    if config.tls.enabled {
        std::env::set_var("SPINE_TLS", "true");
    }
    std::env::set_var("SPINE_LOG_LEVEL", &config.logging.level);
    std::env::set_var("SPINE_LOG_FORMAT", &config.logging.format);
    std::env::set_var("SPINE_MAX_SESSIONS", config.server.max_sessions.to_string());
    std::env::set_var("SPINE_REGION", &config.cluster.region);

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
        let mut child = tokio::process::Command::new(&core_bin)
            .kill_on_drop(true)
            .spawn()?;
        let status = child.wait().await?;
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
