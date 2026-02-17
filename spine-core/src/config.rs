//! SPINE Server Configuration
//!
//! Loads configuration from `spine.toml` (if present), environment variables,
//! and command-line defaults. Environment variables override file values.
//!
//! # Priority (highest to lowest)
//! 1. Environment variables (`SPINE_PORT`, `SPINE_TLS`, etc.)
//! 2. `spine.toml` in working directory
//! 3. Built-in defaults
//!
//! # Example `spine.toml`
//! ```toml
//! [server]
//! host = "0.0.0.0"
//! port = 8080
//! ws_port_offset = 1
//! quic_port_offset = 2
//! metrics_port = 9090
//! max_sessions = 1000
//! max_connections_per_ip = 50
//! idle_timeout_secs = 300
//! session_watchdog_secs = 600
//!
//! [tls]
//! enabled = false
//! cert_path = "certs/cert.pem"
//! key_path = "certs/key.pem"
//! ca_path = "certs/ca.pem"
//!
//! [cluster]
//! port_offset = 1000
//! region = "us-west"
//! skills = ["research", "synthesis", "scraping"]
//!
//! [logging]
//! format = "json"
//! level = "info"
//! ```

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SpineConfig {
    pub server: ServerConfig,
    pub tls: TlsConfig,
    pub cluster: ClusterConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// WebSocket listener port = port + ws_port_offset
    pub ws_port_offset: u16,
    /// QUIC listener port = port + quic_port_offset
    pub quic_port_offset: u16,
    /// Metrics/dashboard HTTP port
    pub metrics_port: u16,
    /// Maximum concurrent sessions
    pub max_sessions: usize,
    /// Maximum connections from a single IP
    pub max_connections_per_ip: usize,
    /// Idle connection timeout (seconds)
    pub idle_timeout_secs: u64,
    /// Session watchdog interval (seconds) — kills sessions with no activity
    pub session_watchdog_secs: u64,
    /// Session persistence interval (seconds)
    pub persistence_interval_secs: u64,
    /// Graceful shutdown timeout (seconds)
    pub shutdown_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TlsConfig {
    pub enabled: bool,
    pub cert_path: String,
    pub key_path: String,
    pub ca_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClusterConfig {
    pub port_offset: u16,
    pub region: String,
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// "json" or "pretty"
    pub format: String,
    /// "trace", "debug", "info", "warn", "error"
    pub level: String,
}

// ========== DEFAULTS ==========

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8080,
            ws_port_offset: 1,
            quic_port_offset: 2,
            metrics_port: 9090,
            max_sessions: 1000,
            max_connections_per_ip: 50,
            idle_timeout_secs: 300,
            session_watchdog_secs: 600,
            persistence_interval_secs: 60,
            shutdown_timeout_secs: 30,
        }
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cert_path: "certs/cert.pem".into(),
            key_path: "certs/key.pem".into(),
            ca_path: "certs/ca.pem".into(),
        }
    }
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            port_offset: 1000,
            region: "us-west".into(),
            skills: vec!["research".into(), "synthesis".into(), "scraping".into()],
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            format: "pretty".into(),
            level: "info".into(),
        }
    }
}

impl SpineConfig {
    /// Load configuration: spine.toml → env overrides → defaults.
    pub fn load() -> Self {
        let mut config = if Path::new("spine.toml").exists() {
            match std::fs::read_to_string("spine.toml") {
                Ok(contents) => match toml::from_str::<SpineConfig>(&contents) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("Warning: failed to parse spine.toml: {e}. Using defaults.");
                        Self::default()
                    }
                },
                Err(e) => {
                    eprintln!("Warning: failed to read spine.toml: {e}. Using defaults.");
                    Self::default()
                }
            }
        } else {
            Self::default()
        };

        // Environment variable overrides
        if let Ok(v) = std::env::var("SPINE_HOST") {
            config.server.host = v;
        }
        if let Ok(v) = std::env::var("PORT").or_else(|_| std::env::var("SPINE_PORT")) {
            if let Ok(p) = v.parse() {
                config.server.port = p;
            }
        }
        if let Ok(v) = std::env::var("SPINE_METRICS_PORT") {
            if let Ok(p) = v.parse() {
                config.server.metrics_port = p;
            }
        }
        if let Ok(v) = std::env::var("SPINE_MAX_SESSIONS") {
            if let Ok(n) = v.parse() {
                config.server.max_sessions = n;
            }
        }
        if let Ok(v) = std::env::var("SPINE_TLS") {
            config.tls.enabled = v == "1" || v.eq_ignore_ascii_case("true");
        }
        if let Ok(v) = std::env::var("SPINE_LOG_FORMAT") {
            config.logging.format = v;
        }
        if let Ok(v) = std::env::var("SPINE_LOG_LEVEL") {
            config.logging.level = v;
        }
        if let Ok(v) = std::env::var("SPINE_REGION") {
            config.cluster.region = v;
        }
        if let Ok(v) = std::env::var("SPINE_IDLE_TIMEOUT") {
            if let Ok(s) = v.parse() {
                config.server.idle_timeout_secs = s;
            }
        }
        if let Ok(v) = std::env::var("SPINE_SHUTDOWN_TIMEOUT") {
            if let Ok(s) = v.parse() {
                config.server.shutdown_timeout_secs = s;
            }
        }

        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SpineConfig::default();
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.server.max_sessions, 1000);
        assert!(!config.tls.enabled);
        assert_eq!(config.logging.format, "pretty");
    }

    #[test]
    fn test_toml_roundtrip() {
        let config = SpineConfig::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: SpineConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.server.port, config.server.port);
        assert_eq!(deserialized.server.max_sessions, config.server.max_sessions);
    }

    #[test]
    fn test_partial_toml() {
        let fragment = r#"
[server]
port = 3000
max_sessions = 500
"#;
        let config: SpineConfig = toml::from_str(fragment).unwrap();
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.server.max_sessions, 500);
        // Rest should be defaults
        assert!(!config.tls.enabled);
        assert_eq!(config.server.host, "127.0.0.1");
    }
}
