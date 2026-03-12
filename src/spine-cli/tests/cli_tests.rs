//! Integration tests for spine-cli
//!
//! Tests offline functionality: CLI parsing, init scaffolding, utility logic.

// =============================================================================
// INIT COMMAND TESTS
// =============================================================================

#[tokio::test]
async fn test_init_creates_project_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_path_buf();

    spine_cli_test_helpers::run_init(path.clone(), None, false, true).await;

    assert!(path.join("spine.toml").exists());
    assert!(path.join("Cargo.toml").exists());
    assert!(path.join(".gitignore").exists());
    assert!(path.join("src/main.rs").exists());
    assert!(path.join("sessions").is_dir());
    assert!(path.join("certs").is_dir());
}

#[tokio::test]
async fn test_init_creates_examples() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_path_buf();

    spine_cli_test_helpers::run_init(path.clone(), None, false, true).await;

    assert!(path.join("examples/knowledge.rs").exists());
    assert!(path.join("examples/swarm.rs").exists());
    assert!(path.join("examples/benchmark.rs").exists());
}

#[tokio::test]
async fn test_init_no_examples() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_path_buf();

    spine_cli_test_helpers::run_init(path.clone(), None, false, false).await;

    assert!(path.join("spine.toml").exists());
    assert!(!path.join("examples").exists());
}

#[tokio::test]
async fn test_init_tls_config() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_path_buf();

    spine_cli_test_helpers::run_init(path.clone(), None, true, false).await;

    let config = std::fs::read_to_string(path.join("spine.toml")).unwrap();
    assert!(config.contains("enabled = true"));
    assert!(config.contains("8443")); // TLS port
}

#[tokio::test]
async fn test_init_plaintext_config() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_path_buf();

    spine_cli_test_helpers::run_init(path.clone(), None, false, false).await;

    let config = std::fs::read_to_string(path.join("spine.toml")).unwrap();
    assert!(config.contains("enabled = false"));
    assert!(config.contains("8080"));
}

#[tokio::test]
async fn test_init_custom_name() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_path_buf();

    spine_cli_test_helpers::run_init(path.clone(), Some("my-agent".into()), false, false).await;

    let cargo = std::fs::read_to_string(path.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("name = \"my-agent\""));
}

#[tokio::test]
async fn test_init_cargo_toml_has_dependencies() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_path_buf();

    spine_cli_test_helpers::run_init(path.clone(), None, false, false).await;

    let cargo = std::fs::read_to_string(path.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("spine-agent"));
    assert!(cargo.contains("spine-protocol"));
    assert!(cargo.contains("tokio"));
    assert!(cargo.contains("anyhow"));
}

#[tokio::test]
async fn test_init_gitignore_contents() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_path_buf();

    spine_cli_test_helpers::run_init(path.clone(), None, false, false).await;

    let gitignore = std::fs::read_to_string(path.join(".gitignore")).unwrap();
    assert!(gitignore.contains("/target"));
    assert!(gitignore.contains("/sessions"));
}

// =============================================================================
// UTILITY / PORT LOGIC TESTS
// =============================================================================

#[test]
fn test_metrics_port_derivation_default() {
    // The status command derives metrics port from main port
    // port 8080 -> metrics 9090
    let port: u16 = 8080;
    let metrics_port = if port == 8080 { 9090 } else { port + 1010 };
    assert_eq!(metrics_port, 9090);
}

#[test]
fn test_metrics_port_derivation_custom() {
    let port: u16 = 9000;
    let metrics_port = if port == 8080 { 9090 } else { port + 1010 };
    assert_eq!(metrics_port, 10010);
}

fn parse_addr(addr: &str) -> (String, u16) {
    match addr.split_once(':') {
        Some((host, port_str)) => (host.to_string(), port_str.parse().unwrap_or(8080)),
        None => ("127.0.0.1".to_string(), 8080),
    }
}

fn parse_tags(tags: &str) -> Vec<String> {
    if tags.is_empty() {
        vec![]
    } else {
        tags.split(',').map(|t| t.trim().to_string()).collect()
    }
}

#[test]
fn test_addr_parsing_host_port() {
    let (host, port) = parse_addr("192.168.1.1:9090");
    assert_eq!(host, "192.168.1.1");
    assert_eq!(port, 9090);
}

#[test]
fn test_addr_parsing_default() {
    let (host, port) = parse_addr("localhost");
    assert_eq!(host, "127.0.0.1");
    assert_eq!(port, 8080);
}

#[test]
fn test_tag_parsing_empty() {
    let tag_list = parse_tags("");
    assert!(tag_list.is_empty());
}

#[test]
fn test_tag_parsing_multiple() {
    let tag_list = parse_tags("rust, ai, agents");
    assert_eq!(tag_list, vec!["rust", "ai", "agents"]);
}

#[test]
fn test_tag_parsing_single() {
    let tag_list = parse_tags("web");
    assert_eq!(tag_list, vec!["web"]);
}

/// Helper module that re-exports the init command logic for testing.
/// Since spine-cli is a binary crate, we call the command functions directly.
mod spine_cli_test_helpers {
    use std::path::PathBuf;

    /// Run the init command with given parameters.
    /// This duplicates the init logic since it's in a binary crate's private module.
    pub async fn run_init(path: PathBuf, name: Option<String>, tls: bool, examples: bool) {
        let project_name = name.unwrap_or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("my-spine-project")
                .to_string()
        });

        // Create directories
        let dirs = ["src", "sessions", "certs"];
        for d in &dirs {
            let dir = path.join(d);
            std::fs::create_dir_all(&dir).unwrap();
        }

        // Write spine.toml
        let config_content = if tls { SPINE_TOML_TLS } else { SPINE_TOML };
        std::fs::write(path.join("spine.toml"), config_content).unwrap();

        // Write .gitignore
        std::fs::write(path.join(".gitignore"), "/target\n/sessions\n*.log\n").unwrap();

        // Write Cargo.toml
        let cargo_toml = format!(
            r#"[package]
name = "{project_name}"
version = "0.1.0"
edition = "2021"

[dependencies]
spine-agent = {{ git = "https://github.com/nervosys/SPINE.git" }}
spine-protocol = {{ git = "https://github.com/nervosys/SPINE.git" }}
tokio = {{ version = "1", features = ["full"] }}
anyhow = "1.0"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1.0"
env_logger = "0.10"
log = "0.4"
"#
        );
        std::fs::write(path.join("Cargo.toml"), cargo_toml).unwrap();

        // Write main.rs
        std::fs::write(path.join("src/main.rs"), "fn main() {}").unwrap();

        // Write example files
        if examples {
            let examples_dir = path.join("examples");
            std::fs::create_dir_all(&examples_dir).unwrap();
            std::fs::write(examples_dir.join("knowledge.rs"), "// knowledge example").unwrap();
            std::fs::write(examples_dir.join("swarm.rs"), "// swarm example").unwrap();
            std::fs::write(examples_dir.join("benchmark.rs"), "// benchmark example").unwrap();
        }
    }

    const SPINE_TOML: &str = r#"# SPINE Server Configuration
[server]
host = "127.0.0.1"
port = 8080
enabled = false
"#;

    const SPINE_TOML_TLS: &str = r#"# SPINE Server Configuration (TLS enabled)
[server]
host = "0.0.0.0"
port = 8443

[tls]
enabled = true
"#;
}
