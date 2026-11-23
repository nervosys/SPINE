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
//! hello_timeout_secs = 10
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
//!
//! # Optional: the Ed25519 public key (64 hex characters) each seed must prove.
//! # A seed without an entry is trusted on first contact and then pinned for the
//! # life of the process.
//! [namespace.seed_keys]
//! "10.0.0.7:9440" = "4f1c0a9e5d3b7a8c2e6f1d0b9a8c7e5d4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c"
//!
//! # Layer-3 authority. Only "off" is accepted by this build: spine-wire v1
//! # cannot carry a grant chain, so nothing would be enforced.
//! [authority]
//! mode = "off"
//! trusted_roots = []
//! skew_secs = 0
//! max_invocation_age_secs = 60
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
    pub outbound: crate::outbound::FetchConfig,
    pub authority: AuthorityConfig,
}

/// Whether the node demands a grant chain for every command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthorityMode {
    /// No grant is asked for: every connected client has full control of the
    /// node, as before layer 3 existed. The default.
    #[default]
    Off,
    /// Every command needs a valid grant chain and invocation. Refused by
    /// [`SpineConfig::validate`] in this build: spine-wire v1 has no field
    /// that carries them, so the daemon could not enforce it.
    Required,
}

/// The largest value `authority.max_invocation_age_secs` may take, in
/// seconds. The ledger remembers every nonce for at least this long.
pub const MAX_INVOCATION_AGE_SECS: u64 = 3600;

/// Layer-3 authority (`docs/spec/03-authority.md`, "Node enforcement").
///
/// These settings are parsed and validated but not acted on: the daemon does
/// not check grants yet. Unknown keys in this section are refused, so a
/// misspelled key cannot be mistaken for a setting that took effect.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthorityConfig {
    /// `"off"` (default) or `"required"`. `"required"` is refused at startup
    /// in this build.
    pub mode: AuthorityMode,
    /// Ed25519 public keys (64 hex characters each, as in
    /// `namespace.seed_keys`) accepted as the root of a chain, in addition to
    /// the node's own key. Commands are classified as operations on the
    /// node's own resource tree, which only the node's key can root, so
    /// these keys have no effect until commands carry resources owned by
    /// other keys.
    pub trusted_roots: Vec<String>,
    /// Clock skew tolerated at the start of a validity window and for an
    /// invocation dated in the future, at most
    /// `spine_authority::MAX_CLOCK_SKEW_SECS` (300).
    pub skew_secs: u64,
    /// Oldest an invocation may be, 1 to [`MAX_INVOCATION_AGE_SECS`].
    pub max_invocation_age_secs: u64,
}

impl Default for AuthorityConfig {
    fn default() -> Self {
        Self {
            mode: AuthorityMode::Off,
            trusted_roots: Vec::new(),
            skew_secs: 0,
            max_invocation_age_secs: 60,
        }
    }
}

impl AuthorityConfig {
    /// The configured root keys, decoded.
    ///
    /// Fails if an entry is not exactly 64 hex characters or appears twice.
    /// Error text does not echo the configured values.
    pub fn trusted_root_keys(&self) -> anyhow::Result<Vec<[u8; 32]>> {
        let mut keys: Vec<[u8; 32]> = Vec::with_capacity(self.trusted_roots.len());
        for text in &self.trusted_roots {
            let key = if text.bytes().all(|b| b.is_ascii_hexdigit()) {
                parse_key_hex(text)
            } else {
                None
            };
            let key = key.ok_or_else(|| {
                anyhow::anyhow!("authority.trusted_roots values must be 64 hex characters")
            })?;
            anyhow::ensure!(
                !keys.contains(&key),
                "authority.trusted_roots lists a key twice"
            );
            keys.push(key);
        }
        Ok(keys)
    }

    /// Checks the section. `mode = "required"` is refused: see
    /// [`AuthorityMode::Required`].
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.mode != AuthorityMode::Required,
            "authority.mode = \"required\" is not available in this build: spine-wire v1 has no \
             field that carries a grant chain or an invocation, so the daemon cannot check one \
             and would enforce nothing; set authority.mode = \"off\""
        );
        self.trusted_root_keys()?;
        anyhow::ensure!(
            self.skew_secs <= spine_authority::MAX_CLOCK_SKEW_SECS,
            "authority.skew_secs must be at most 300"
        );
        anyhow::ensure!(
            (1..=MAX_INVOCATION_AGE_SECS).contains(&self.max_invocation_age_secs),
            "authority.max_invocation_age_secs must be 1..=3600"
        );
        Ok(())
    }
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
    /// Optional DNS host:port bootstrap sources, resolved with bounded fan-out.
    pub dns_seeds: Vec<String>,
    /// Expected mesh keys for seeds, as a table from the seed address to the
    /// seed's Ed25519 public key (64 hex characters; the same bytes as its
    /// keyspace id). The address is spelled as in `seeds` (a bare `host:port`
    /// or a `spine://host:h:p/` name) and must appear there.
    ///
    /// A seed with a key here must prove exactly that key on every dial, so a
    /// wrong or replaced node at the address is refused at first contact. A seed
    /// without one is trusted on first contact: its key is recorded for the life
    /// of the process and then required on every later dial to that address.
    /// Addresses found through `dns_seeds` have no key and are always in the
    /// second case.
    pub seed_keys: std::collections::BTreeMap<String, String>,
    /// Enable durable read-only task and knowledge RPC for explicitly allowed keys.
    pub tasks_enabled: bool,
    pub task_peers: Vec<String>,
    /// Separate inbound and outbound TCP mesh connection budgets (1..=1024).
    pub connection_limit: usize,
    pub handshake_timeout_secs: u64,
    /// Complete frame deadline, including idle time before its header.
    pub frame_timeout_secs: u64,
    pub queue_timeout_secs: u64,
    pub write_timeout_secs: u64,
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
    /// Durable local publications. Empty disables namespace persistence.
    pub records_path: String,
    /// Re-sign only records owned by this node's key. Off unless opted in.
    pub renew_owned_records: bool,
    /// Maximum time between owner renewal checks.
    pub renewal_secs: u64,
    /// How often, in seconds, to re-offer held records to the nodes now closest
    /// to them and drop the expired.
    ///
    /// This is churn insurance, not renewal: a record's expiry is signed into it
    /// and cannot be extended by re-announcing. Too long and records drift away
    /// from the keyspace position lookups converge on; too short and every node
    /// runs a keyspace walk per held record more often than the mesh changes.
    pub maintain_secs: u64,
    /// How close to expiry, in seconds, a name has to be before it is reported
    /// as lapsing. Only its signing key's holder can renew it.
    pub lapse_window_secs: u64,
    /// Most records to re-offer in one maintenance pass.
    ///
    /// Each costs a keyspace walk, so this bounds what a node with a large
    /// store does to the mesh on every tick. Passes resume where the last one
    /// stopped, so a store larger than the batch is covered across several
    /// ticks rather than having its tail starved.
    pub maintain_batch: usize,
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
    /// Global native TCP/WebSocket/QUIC connections, including handshakes.
    pub max_connections: usize,
    /// Maximum connections from a single IP
    pub max_connections_per_ip: usize,
    /// Idle connection timeout (seconds)
    pub idle_timeout_secs: u64,
    /// Seconds a native connection has to send `HELLO` (spine-wire v1, C.6)
    /// before the daemon closes it with `CLOSE(timeout)`. A client that
    /// connects and says nothing holds one admission slot for this long.
    pub hello_timeout_secs: u64,
    /// Session watchdog interval (seconds) — kills sessions with no activity
    pub session_watchdog_secs: u64,
    /// Retain detached sessions for this many seconds; connected sessions are protected.
    pub session_retention_secs: u64,
    /// Session persistence interval (seconds)
    pub persistence_interval_secs: u64,
    /// Graceful shutdown timeout (seconds)
    pub shutdown_timeout_secs: u64,
    /// Accept native clients from beyond loopback without mutual TLS.
    ///
    /// The native server has no per-client authorization: every connected
    /// client can publish names and resources, call tools, and make this node
    /// sign mesh calls with its own key. Until delegated authority exists, a
    /// non-loopback `host` is refused at startup unless `tls.mutual_tls` is on
    /// or this is set. Setting it states that the network itself is trusted.
    pub allow_unauthenticated_remote: bool,
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
            max_connections: 256,
            max_connections_per_ip: 50,
            idle_timeout_secs: 300,
            hello_timeout_secs: 10,
            session_watchdog_secs: 600,
            session_retention_secs: 3600,
            persistence_interval_secs: 60,
            shutdown_timeout_secs: 30,
            allow_unauthenticated_remote: false,
        }
    }
}

/// Whether a configured listener host is reachable only from this machine.
fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
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
            dns_seeds: Vec::new(),
            seed_keys: std::collections::BTreeMap::new(),
            advertise: Vec::new(),
            enabled: false,
            port: 9440,
            key_path: "spine-node.key".into(),
            records_path: "spine-names.sqlite".into(),
            tasks_enabled: false,
            task_peers: Vec::new(),
            connection_limit: 64,
            handshake_timeout_secs: 10,
            frame_timeout_secs: 60,
            queue_timeout_secs: 5,
            write_timeout_secs: 10,
            renew_owned_records: false,
            renewal_secs: 60,
            // Kademlia's hourly replication interval, and for the same reason:
            // it is short relative to how fast a neighbourhood turns over and
            // long relative to how long a walk takes.
            maintain_secs: 3600,
            lapse_window_secs: 900,
            maintain_batch: 64,
        }
    }
}

impl NamespaceConfig {
    pub fn connection_limits(&self) -> spine_mesh::mesh_tcp::ConnectionLimits {
        use std::time::Duration;
        spine_mesh::mesh_tcp::ConnectionLimits {
            max_connections: self.connection_limit,
            handshake_timeout: Duration::from_secs(self.handshake_timeout_secs),
            frame_timeout: Duration::from_secs(self.frame_timeout_secs),
            queue_timeout: Duration::from_secs(self.queue_timeout_secs),
            write_timeout: Duration::from_secs(self.write_timeout_secs),
        }
    }

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

    /// Seeds configured with an expected key, as `(dialable address, key)`.
    ///
    /// Fails if a key is not 64 hex characters or its address is not one of
    /// the configured `seeds`: a pin that can never apply would be a silent
    /// misconfiguration. Error text does not echo the configured values.
    pub fn seed_pins(&self) -> anyhow::Result<Vec<(String, [u8; 32])>> {
        let seeds = self.seed_addresses();
        let mut pins = Vec::new();
        for (address, key) in &self.seed_keys {
            let address = host_address(address).unwrap_or_else(|| address.clone());
            anyhow::ensure!(
                seeds.contains(&address),
                "namespace.seed_keys has an entry whose address is not in namespace.seeds"
            );
            let key = parse_key_hex(key).ok_or_else(|| {
                anyhow::anyhow!("namespace.seed_keys values must be 64 hex characters")
            })?;
            pins.push((address, key));
        }
        Ok(pins)
    }
}

/// Decode a 32-byte key from 64 hex characters.
fn parse_key_hex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
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
    #[allow(dead_code)] // Compatibility entry point; executables use fallible try_load.
    pub fn load() -> Self {
        Self::try_load().expect("SPINE configuration is invalid")
    }

    /// Parse an explicitly selected file without environment changes or copying
    /// over the working directory's configuration. Error messages omit content.
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)
            .map_err(|_| anyhow::anyhow!("could not read configuration {}", path.display()))?;
        let config: Self = toml::from_str(&contents)
            .map_err(|_| anyhow::anyhow!("invalid TOML configuration {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        self.outbound.validate()?;
        self.authority.validate()?;
        self.namespace
            .connection_limits()
            .validate()
            .map_err(|_| anyhow::anyhow!("invalid namespace connection limits"))?;
        anyhow::ensure!(
            (1..=65536).contains(&self.server.max_sessions)
                && self.server.max_connections_per_ip > 0
                && self.server.max_connections > 0
                && self.server.max_connections <= 65536,
            "connection and session limits must be positive and within configured bounds"
        );
        anyhow::ensure!(
            self.server.session_watchdog_secs > 0
                && (1..=2_592_000).contains(&self.server.session_retention_secs)
                && self.server.persistence_interval_secs > 0
                && self.server.idle_timeout_secs > 0
                && self.server.hello_timeout_secs > 0,
            "maintenance intervals must be positive; session retention must be 1..=2592000 seconds"
        );
        anyhow::ensure!(
            !self.tls.mutual_tls || (self.tls.enabled && !self.tls.ca_path.is_empty()),
            "mutual TLS requires enabled TLS and an explicit CA path"
        );
        anyhow::ensure!(
            self.tls.crl_path.is_empty() || self.tls.mutual_tls,
            "a client revocation list requires mutual TLS"
        );
        anyhow::ensure!(
            is_loopback_host(&self.server.host)
                || self.tls.mutual_tls
                || self.server.allow_unauthenticated_remote,
            "server.host is not loopback: every native client has full control of this node, \
             so enable tls.mutual_tls or set server.allow_unauthenticated_remote = true"
        );
        for offset in [
            self.server.ws_port_offset,
            self.server.quic_port_offset,
            self.cluster.port_offset,
        ] {
            anyhow::ensure!(
                self.server.port.checked_add(offset).is_some(),
                "listener port plus offset exceeds 65535"
            );
        }
        anyhow::ensure!(
            self.server.metrics_port < u16::MAX,
            "metrics port leaves no room for the HTTP gateway"
        );
        anyhow::ensure!(
            !self.namespace.tasks_enabled
                || (self.namespace.enabled && !self.namespace.task_peers.is_empty()),
            "task worker requires an enabled namespace and explicit peer keys"
        );
        self.namespace.seed_pins()?;
        Ok(())
    }

    pub fn try_load() -> anyhow::Result<Self> {
        let mut config = if let Some(path) = std::env::var_os("SPINE_CONFIG_PATH") {
            Self::from_file(Path::new(&path))?
        } else if Path::new("spine.toml").exists() {
            Self::from_file(Path::new("spine.toml"))?
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
        if let Ok(v) = std::env::var("SPINE_HELLO_TIMEOUT") {
            if let Ok(s) = v.parse() {
                config.server.hello_timeout_secs = s;
            }
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

        config.validate()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_explicit_configuration_is_refused_without_disclosing_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("selected.toml");
        std::fs::write(&path, "private-marker = [ invalid").unwrap();
        let error = SpineConfig::from_file(&path).unwrap_err().to_string();
        assert!(!error.contains("private-marker"));
        assert!(SpineConfig::from_file(&dir.path().join("missing.toml")).is_err());
        let mut config = SpineConfig::default();
        config.server.port = u16::MAX;
        assert!(config.validate().is_err());
        config.server.port = 8080;
        config.server.persistence_interval_secs = 0;
        assert!(config.validate().is_err());
        config.server.persistence_interval_secs = 60;
        config.server.session_retention_secs = 0;
        assert!(config.validate().is_err());
        config.server.session_retention_secs = 2_592_001;
        assert!(config.validate().is_err());
        config.server.session_retention_secs = 3600;
        config.server.max_sessions = usize::MAX;
        assert!(config.validate().is_err());
        config.server.max_sessions = 1000;
        config.namespace.connection_limit = 0;
        assert!(config.validate().is_err());
        config.namespace.connection_limit = 64;
        config.tls.mutual_tls = true;
        assert!(config.validate().is_err());
        config.tls.enabled = true;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn operator_examples_parse_with_validated_bounds() {
        for example in [
            "single-node.toml",
            "mesh-worker.toml",
            "gateway.toml",
            "irongate.toml",
            "mutual-tls.toml",
            "internal-navigation.toml",
        ] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/config")
                .join(example);
            let cfg = SpineConfig::from_file(&path).unwrap();
            assert!(cfg.validate().is_ok());
        }
    }

    #[test]
    fn test_default_config() {
        let config = SpineConfig::default();
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.server.max_sessions, 1000);
        assert!(!config.tls.enabled);
        assert_eq!(config.logging.format, "pretty");
        assert_eq!(config.server.hello_timeout_secs, 10);
    }

    #[test]
    fn the_hello_deadline_is_configurable_and_must_be_positive() {
        let config: SpineConfig = toml::from_str(
            r#"
[server]
hello_timeout_secs = 3
"#,
        )
        .unwrap();
        assert_eq!(config.server.hello_timeout_secs, 3);
        assert!(config.validate().is_ok());

        let config = SpineConfig {
            server: ServerConfig {
                hello_timeout_secs: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
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

    /// A seed key is a 64-hex Ed25519 key for an address that is in `seeds`;
    /// anything else is a configuration error, not a pin that never applies.
    #[test]
    fn seed_keys_must_be_hex_keys_for_listed_seeds() {
        let key = "ab".repeat(32);
        let good = format!(
            "[namespace]\nenabled = true\nseeds = [\"spine://host:seed.example.org:9440/\", \
             \"10.0.0.7:9440\"]\n[namespace.seed_keys]\n\"10.0.0.7:9440\" = \"{key}\"\n\
             \"spine://host:seed.example.org:9440/\" = \"{key}\"\n"
        );
        let config: SpineConfig = toml::from_str(&good).unwrap();
        assert!(config.validate().is_ok());
        let pins = config.namespace.seed_pins().unwrap();
        assert_eq!(pins.len(), 2);
        assert!(pins.iter().all(|(_, k)| k == &[0xab; 32]));
        assert!(pins.iter().any(|(a, _)| a == "seed.example.org:9440"));

        let bad_hex = good.replace(&key, "not-hex");
        let config: SpineConfig = toml::from_str(&bad_hex).unwrap();
        assert!(config.validate().is_err());

        let unlisted = format!(
            "[namespace]\nenabled = true\nseeds = [\"10.0.0.7:9440\"]\n\
             [namespace.seed_keys]\n\"10.9.9.9:9440\" = \"{key}\"\n"
        );
        let config: SpineConfig = toml::from_str(&unlisted).unwrap();
        assert!(config.validate().is_err());
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
    fn a_remote_listener_needs_client_authentication_or_an_explicit_override() {
        let mut config = SpineConfig::default();
        assert!(config.validate().is_ok(), "loopback default is accepted");

        for host in ["localhost", "127.0.0.1", "::1", "[::1]"] {
            config.server.host = host.into();
            assert!(config.validate().is_ok(), "{host}");
        }

        for host in ["0.0.0.0", "::", "192.0.2.10", "node.example.org"] {
            config.server.host = host.into();
            assert!(config.validate().is_err(), "{host}");
        }

        config.server.allow_unauthenticated_remote = true;
        assert!(config.validate().is_ok(), "explicit override is accepted");
    }

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
    fn authority_defaults_to_off_and_an_absent_section_is_the_default() {
        let config: SpineConfig = toml::from_str("[server]\nport = 3000\n").unwrap();
        assert_eq!(config.authority.mode, AuthorityMode::Off);
        assert!(config.authority.trusted_roots.is_empty());
        assert_eq!(config.authority.skew_secs, 0);
        assert_eq!(config.authority.max_invocation_age_secs, 60);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn an_authority_section_parses() {
        let key = "ab".repeat(32);
        let text = format!(
            "[authority]\nmode = \"off\"\ntrusted_roots = [\"{key}\"]\nskew_secs = 30\n\
             max_invocation_age_secs = 120\n"
        );
        let config: SpineConfig = toml::from_str(&text).unwrap();
        assert_eq!(config.authority.mode, AuthorityMode::Off);
        assert_eq!(config.authority.skew_secs, 30);
        assert_eq!(config.authority.max_invocation_age_secs, 120);
        assert_eq!(
            config.authority.trusted_root_keys().unwrap(),
            vec![[0xabu8; 32]]
        );
        assert!(config.validate().is_ok());
    }

    /// Enforcement is not connected to the wire, so a configuration that asks
    /// for it must stop the daemon instead of starting one that enforces
    /// nothing.
    #[test]
    fn authority_mode_required_is_refused_in_this_build() {
        let config: SpineConfig = toml::from_str("[authority]\nmode = \"required\"\n").unwrap();
        assert_eq!(config.authority.mode, AuthorityMode::Required);
        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("authority.mode"), "{error}");
        assert!(error.contains("not available in this build"), "{error}");
    }

    #[test]
    fn an_unknown_authority_mode_or_key_does_not_parse() {
        assert!(toml::from_str::<SpineConfig>("[authority]\nmode = \"on\"\n").is_err());
        assert!(toml::from_str::<SpineConfig>("[authority]\nmode = \"Required\"\n").is_err());
        assert!(toml::from_str::<SpineConfig>("[authority]\nmod = \"off\"\n").is_err());
    }

    #[test]
    fn authority_trusted_roots_must_be_distinct_hex_keys() {
        let good = "0f".repeat(32);
        for bad in [
            "not-hex".to_string(),
            "ab".repeat(31),
            "ab".repeat(33),
            format!("+f{}", "ab".repeat(31)),
            format!("zz{}", "ab".repeat(31)),
            String::new(),
        ] {
            let config = SpineConfig {
                authority: AuthorityConfig {
                    trusted_roots: vec![good.clone(), bad.clone()],
                    ..Default::default()
                },
                ..Default::default()
            };
            let error = config.validate().unwrap_err().to_string();
            assert!(error.contains("authority.trusted_roots"), "{error}");
            assert!(bad.is_empty() || !error.contains(&bad), "{error}");
        }
        let twice = SpineConfig {
            authority: AuthorityConfig {
                trusted_roots: vec![good.clone(), good.to_uppercase()],
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(twice.validate().is_err());
    }

    #[test]
    fn authority_skew_and_invocation_age_are_bounded() {
        let with = |skew_secs: u64, max_invocation_age_secs: u64| SpineConfig {
            authority: AuthorityConfig {
                skew_secs,
                max_invocation_age_secs,
                ..Default::default()
            },
            ..Default::default()
        };
        let clamp = spine_authority::MAX_CLOCK_SKEW_SECS;
        assert!(with(clamp, 60).validate().is_ok());
        assert!(with(clamp + 1, 60).validate().is_err());
        assert!(with(0, 0).validate().is_err());
        assert!(with(0, MAX_INVOCATION_AGE_SECS).validate().is_ok());
        assert!(with(0, MAX_INVOCATION_AGE_SECS + 1).validate().is_err());
    }

    #[test]
    fn test_toml_roundtrip() {
        let config = SpineConfig::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: SpineConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.server.port, config.server.port);
        assert_eq!(deserialized.server.max_sessions, config.server.max_sessions);
        assert_eq!(deserialized.authority.mode, AuthorityMode::Off);
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
