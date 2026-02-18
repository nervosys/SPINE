//! SPINE CLI — Command-line interface for the SPINE agentic web stack
//!
//! # Commands
//!
//! - `spine init` — Scaffold a new SPINE project with config and examples
//! - `spine connect <addr>` — Connect to a SPINE server and start interactive session
//! - `spine query <subcommand>` — Send queries (navigate, search, knowledge)
//! - `spine deploy` — Start a SPINE server from config
//! - `spine benchmark <addr>` — Run performance benchmarks against a server
//! - `spine status <addr>` — Check server health and metrics

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;

/// SPINE — The Agentic Web Stack
#[derive(Parser)]
#[command(name = "spine", version, about = "CLI for the SPINE agentic web stack")]
#[command(propagate_version = true)]
struct Cli {
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new SPINE project
    Init {
        /// Project directory (default: current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Project name
        #[arg(short, long)]
        name: Option<String>,

        /// Enable TLS configuration
        #[arg(long)]
        tls: bool,

        /// Include example agent code
        #[arg(long, default_value_t = true)]
        examples: bool,
    },

    /// Connect to a SPINE server
    Connect {
        /// Server address (host:port)
        #[arg(default_value = "127.0.0.1:8080")]
        addr: String,

        /// Use WebSocket transport (ws:// or wss://)
        #[arg(long)]
        ws: bool,

        /// Use TLS
        #[arg(long)]
        tls: bool,

        /// TLS CA certificate path
        #[arg(long)]
        ca: Option<PathBuf>,

        /// TLS domain name
        #[arg(long)]
        domain: Option<String>,

        /// Client certificate path (for mTLS)
        #[arg(long)]
        client_cert: Option<PathBuf>,

        /// Client private key path (for mTLS)
        #[arg(long)]
        client_key: Option<PathBuf>,
    },

    /// Query a SPINE server
    Query {
        #[command(subcommand)]
        action: QueryAction,
    },

    /// Start a SPINE server
    Deploy {
        /// Config file path
        #[arg(short, long, default_value = "spine.toml")]
        config: PathBuf,

        /// Override listen port
        #[arg(short, long)]
        port: Option<u16>,

        /// Override listen host
        #[arg(long)]
        host: Option<String>,
    },

    /// Run performance benchmarks
    Benchmark {
        /// Server address
        #[arg(default_value = "127.0.0.1:8080")]
        addr: String,

        /// Number of iterations
        #[arg(short = 'n', long, default_value_t = 100)]
        iterations: usize,

        /// Number of concurrent connections
        #[arg(short, long, default_value_t = 1)]
        concurrency: usize,
    },

    /// Check server health and metrics
    Status {
        /// Server address
        #[arg(default_value = "127.0.0.1:8080")]
        addr: String,
    },

    /// Certificate management
    #[command(subcommand)]
    Cert(CertAction),
}

#[derive(Subcommand)]
enum CertAction {
    /// Generate development certificates (CA + server + client)
    Generate {
        /// Output directory for certificates
        #[arg(short, long, default_value = "certs")]
        output: PathBuf,
    },

    /// Show certificate information
    Info {
        /// Path to certificate PEM file
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum QueryAction {
    /// Navigate to a URL and fetch its Unified Representation
    Navigate {
        /// URL to navigate to
        url: String,

        /// Server address
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        addr: String,

        /// Output format: text, json, or compact
        #[arg(short, long, default_value = "text")]
        format: String,
    },

    /// Search the semantic web
    Search {
        /// Search query
        query: String,

        /// Server address
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        addr: String,
    },

    /// Store knowledge
    Store {
        /// Key
        key: String,

        /// JSON value
        value: String,

        /// Tags (comma-separated)
        #[arg(short, long, default_value = "")]
        tags: String,

        /// Server address
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        addr: String,
    },

    /// Query knowledge base
    Knowledge {
        /// Query string
        query: String,

        /// Tags filter (comma-separated)
        #[arg(short, long, default_value = "")]
        tags: String,

        /// Max results
        #[arg(short, long, default_value_t = 10)]
        limit: usize,

        /// Server address
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        addr: String,
    },

    /// Get raw HTML from current page
    Html {
        /// URL to fetch
        url: String,

        /// Server address
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        addr: String,
    },

    /// Execute an HLS script
    Exec {
        /// HLS script file or inline code
        script: String,

        /// Server address
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        addr: String,
    },

    /// Ping the server
    Ping {
        /// Server address
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        addr: String,

        /// Number of pings
        #[arg(short = 'n', long, default_value_t = 5)]
        count: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        std::env::set_var("RUST_LOG", "debug");
    } else if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_logger::init();

    match cli.command {
        Commands::Init {
            path,
            name,
            tls,
            examples,
        } => commands::init::run(path, name, tls, examples).await,

        Commands::Connect {
            addr,
            ws,
            tls,
            ca,
            domain,
            client_cert,
            client_key,
        } => commands::connect::run(addr, ws, tls, ca, domain, client_cert, client_key).await,

        Commands::Query { action } => match action {
            QueryAction::Navigate { url, addr, format } => {
                commands::query::navigate(addr, url, format).await
            }
            QueryAction::Search { query, addr } => commands::query::search(addr, query).await,
            QueryAction::Store {
                key,
                value,
                tags,
                addr,
            } => commands::query::store(addr, key, value, tags).await,
            QueryAction::Knowledge {
                query,
                tags,
                limit,
                addr,
            } => commands::query::knowledge(addr, query, tags, limit).await,
            QueryAction::Html { url, addr } => commands::query::html(addr, url).await,
            QueryAction::Exec { script, addr } => commands::query::exec(addr, script).await,
            QueryAction::Ping { addr, count } => commands::query::ping(addr, count).await,
        },

        Commands::Deploy { config, port, host } => commands::deploy::run(config, port, host).await,

        Commands::Benchmark {
            addr,
            iterations,
            concurrency,
        } => commands::benchmark::run(addr, iterations, concurrency).await,

        Commands::Status { addr } => commands::status::run(addr).await,

        Commands::Cert(action) => commands::cert::run(action).await,
    }
}
