//! `spine deploy` — Start a SPINE server

use anyhow::{Context, Result};
use colored::Colorize;
use std::path::PathBuf;

pub async fn run(config_path: PathBuf, port: Option<u16>, host: Option<String>) -> Result<()> {
    // Apply overrides via environment variables (SpineConfig::load reads these)
    if let Some(p) = port {
        std::env::set_var("SPINE_PORT", p.to_string());
    }
    if let Some(h) = &host {
        std::env::set_var("SPINE_HOST", h);
    }

    // Set config file path
    if config_path.as_path() != std::path::Path::new("spine.toml") {
        // Copy to spine.toml in current dir so SpineConfig::load() finds it
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read config: {}", config_path.display()))?;
            std::fs::write("spine.toml", content)?;
        }
    }

    let config = spine_core::SpineConfig::load();
    eprintln!(
        "{} Starting SPINE server on {}:{}",
        "▸".green().bold(),
        config.server.host.cyan(),
        config.server.port.to_string().as_str().cyan()
    );
    if config.tls.enabled {
        eprintln!("  🔒 TLS enabled");
    }
    eprintln!(
        "  Metrics:    http://{}:{}",
        config.server.host, config.server.metrics_port
    );
    eprintln!(
        "  WebSocket:  port {}",
        config.server.port + config.server.ws_port_offset
    );
    eprintln!("  Max sessions: {}", config.server.max_sessions);
    eprintln!();

    // Run the actual server — this calls the same entry point as spine-core's main()
    // We re-use spine_core's serve function if available, otherwise start manually
    spine_core::serve(config).await
}
