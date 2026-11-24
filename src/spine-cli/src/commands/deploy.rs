//! `spine deploy` — Start a SPINE server

use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

pub async fn run(config_path: PathBuf, port: Option<u16>, host: Option<String>) -> Result<()> {
    let mut config = spine_core::SpineConfig::from_file(&config_path)?;
    if let Some(port) = port {
        config.server.port = port;
    }
    if let Some(host) = host {
        config.server.host = host;
    }
    config.validate()?;
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
