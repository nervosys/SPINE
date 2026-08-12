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
//!
//! [namespace]
//! enabled = true
//! seeds = ["spine://host:seed.example.org:9440/", "10.0.0.7:9440"]
//! advertise = ["203.0.113.4:9440"]
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
    pub namespace: NamespaceConfig,
}

/// How this node enters the `spine://` namespace.
///
/// A Kademlia node converges on the right neighbours from any single honest
/// contact, but it cannot acquire the first one by routing — routing is what
/// having a contact enables. That first contact has to come from outside the
/// system, and this is where it comes from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NamespaceConfig {
    /// Seed nodes to dial on startup.
    ///
    /// Accepts either a bare `host:port` or a `spine://host:h:p/` name — the
    /// latter because a seed list is a natural thing to paste from a name, and
    /// silently rejecting the namespace's own spelling for its own bootstrap
    /// addresses would be a poor joke.
    pub seeds: Vec<String>,
    /// Addresses this node advertises to peers it meets.
    ///
    /// Empty means "do not advertise": the node resolves names but is never
    /// offered to anyone else as a contact. That is the right default behind
    /// NAT, where an advertised address would simply fail to dial, and the
    /// wrong one for a node meant to serve as a seed itself.
    pub advertise: Vec<String>,
    /// Whether to join the namespace at all. Off by default: a node with no
    /// seeds configured has nothing to join.
    pub enabled: bool,
    /// Port the mesh listener binds, on the same host as the main server.
    pub port: u16,
    /// Where this node's mesh identity is kept.
    ///
    /// The key is the node's keyspace position, so losing it does not merely
    /// change a credential — it moves the node to a different point in the DHT
    /// and orphans every record published under the old name. It is persisted
    /// for that reason, not for convenience.
    pub key_path: String,
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
    /// Require client certificates (mutual TLS)
    pub mutual_tls: bool,
    /// Path to CRL file for revocation checking
    pub crl_path: String,
    /// Client certificate path (for agent-side mTLS)
    pub client_cert_path: String,
    /// Client key path (for agent-side mTLS)
    pub client_key_path: String,
    /// Certificate reload interval in seconds (0 = disabled)
    pub cert_reload_secs: u64,
    /// Auto-generate self-signed certs for development
    pub auto_generate: bool,
    /// Enable ACME (Let's Encrypt) certificate management
    pub acme_enabled: bool,
    /// ACME domains
    pub acme_domains: Vec<String>,
    /// ACME contact email
    pub acme_email: String,
    /// Use ACME staging environment
    pub acme_staging: bool,
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
            mutual_tls: false,
            crl_path: String::new(),
            client_cert_path: String::new(),
            client_key_path: String::new(),
            cert_reload_secs: 0,
            auto_generate: false,
            acme_enabled: false,
            acme_domains: Vec::new(),
            acme_email: String::new(),
            acme_staging: true,
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

impl Default for NamespaceConfig {
    fn default() -> Self {
        Self {
            seeds: Vec::new(),
            advertise: Vec::new(),
            enabled: false,
            port: 9440,
            key_path: "spine-node.key".into(),
        }
    }
}

impl NamespaceConfig {
    /// The seed list as dialable `host:port` strings.
    ///
    /// Entries spelled as `spine://host:…` names are unwrapped to the address
    /// they contain; anything else is passed through untouched, so a plain
    /// `host:port` works and an unparseable entry fails at dial time with the
    /// text the operator actually wrote rather than being dropped silently here.
    pub fn seed_addresses(&self) -> Vec<String> {
        self.seeds
            .iter()
            .map(|s| host_address(s).unwrap_or_else(|| s.clone()))
            .collect()
    }
}

/// Split a comma- or whitespace-separated env var into non-empty entries.
fn split_list(raw: &str) -> Vec<String> {
    raw.split([',', ' ', '\t', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Extract the `host:port` from a `spine://host:…` name, if that is what it is.
fn host_address(raw: &str) -> Option<String> {
    let uri = spine_name::SpineUri::parse(raw).ok()?;
    match uri.authority() {
        spine_name::Authority::Host { host, port } => Some(match port {
            Some(p) => format!("{host}:{p}"),
            None => host.clone(),
        }),
        _ => None,
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
        if let Ok(v) = std::env::var("SPINE_TLS_CERT") {
            config.tls.cert_path = v;
        }
        if let Ok(v) = std::env::var("SPINE_TLS_KEY") {
            config.tls.key_path = v;
        }
        if let Ok(v) = std::env::var("SPINE_TLS_CA") {
            config.tls.ca_path = v;
        }
        if let Ok(v) = std::env::var("SPINE_TLS_MTLS") {
            config.tls.mutual_tls = v == "1" || v.eq_ignore_ascii_case("true");
        }
        if let Ok(v) = std::env::var("SPINE_TLS_CRL") {
            config.tls.crl_path = v;
        }
        if let Ok(v) = std::env::var("SPINE_TLS_AUTO_GENERATE") {
            config.tls.auto_generate = v == "1" || v.eq_ignore_ascii_case("true");
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
        // Seeds are the one setting a container image cannot bake in, since the
        // mesh a node joins is a deployment fact rather than a build one.
        if let Ok(v) = std::env::var("SPINE_SEEDS") {
            config.namespace.seeds = split_list(&v);
            config.namespace.enabled = !config.namespace.seeds.is_empty();
        }
        if let Ok(v) = std::env::var("SPINE_ADVERTISE") {
            config.namespace.advertise = split_list(&v);
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

    /// A seed list should accept the namespace's own spelling for an address as
    /// readily as a bare one — an operator copying a `spine://host:…` name into
    /// the seed list is doing the obvious thing.
    #[test]
    fn seeds_accept_both_spine_names_and_bare_addresses() {
        let fragment = r#"
[namespace]
enabled = true
seeds = ["spine://host:seed.example.org:9440/", "10.0.0.7:9440"]
"#;
        let config: SpineConfig = toml::from_str(fragment).unwrap();
        assert_eq!(
            config.namespace.seed_addresses(),
            vec!["seed.example.org:9440", "10.0.0.7:9440"]
        );
    }

    /// An entry that is neither must survive to the dial, so the error names
    /// what the operator actually wrote instead of vanishing here.
    #[test]
    fn an_unrecognized_seed_entry_is_passed_through_not_dropped() {
        let config = SpineConfig {
            namespace: NamespaceConfig {
                seeds: vec!["not a name or an address".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            config.namespace.seed_addresses(),
            vec!["not a name or an address"]
        );
    }

    /// A `cap:` or `did:` name carries no address, so it must not be mistaken
    /// for one.
    #[test]
    fn a_non_host_name_is_not_treated_as_an_address() {
        assert_eq!(host_address("spine://cap:web.search/"), None);
    }

    #[test]
    fn a_seed_env_var_splits_on_commas_and_whitespace() {
        assert_eq!(
            split_list(" a:1, b:2\tc:3 "),
            vec!["a:1".to_string(), "b:2".into(), "c:3".into()]
        );
        assert!(split_list("  ,, ").is_empty());
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
