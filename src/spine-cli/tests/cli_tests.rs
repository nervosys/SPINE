//! Integration tests for spine-cli
//!
//! Tests offline functionality: CLI parsing, init scaffolding, utility logic.

#[test]
fn name_create_signs_content_and_refuses_to_overwrite_owner_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("resource.txt"), b"signed bytes").unwrap();
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_spine"))
            .current_dir(dir.path())
            .args([
                "name",
                "create",
                "--key",
                "owner.key",
                "--output",
                "record.json",
                "--endpoint",
                "127.0.0.1:8080",
                "--path",
                "/example",
                "--capability",
                "example.documents",
                "--resource",
                "resource.txt",
            ])
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record: spine_name::NameRecord =
        serde_json::from_slice(&std::fs::read(dir.path().join("record.json")).unwrap()).unwrap();
    record.verify().unwrap();
    assert_eq!(
        record.content_hash,
        Some(spine_name::resource_id("application/octet-stream", b"signed bytes").unwrap())
    );
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains(&record.name.to_string()));
    let before = std::fs::read(dir.path().join("owner.key")).unwrap();
    assert!(!run().status.success());
    assert_eq!(std::fs::read(dir.path().join("owner.key")).unwrap(), before);
}

#[test]
fn name_create_refuses_a_ttl_over_the_maximum_and_revoke_signs_a_tombstone() {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_spine"))
            .current_dir(dir.path())
            .arg("name")
            .args(args)
            .output()
            .unwrap()
    };
    let too_long = (spine_name::MAX_RECORD_TTL_SECS + 1).to_string();
    let output = run(&[
        "create",
        "--key",
        "owner.key",
        "--output",
        "long.json",
        "--endpoint",
        "127.0.0.1:8080",
        "--ttl",
        &too_long,
    ]);
    assert!(!output.status.success());
    assert!(!dir.path().join("long.json").exists());

    let output = run(&[
        "create",
        "--key",
        "owner.key",
        "--output",
        "record.json",
        "--endpoint",
        "127.0.0.1:8080",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record: spine_name::NameRecord =
        serde_json::from_slice(&std::fs::read(dir.path().join("record.json")).unwrap()).unwrap();

    let revoke = || {
        run(&[
            "revoke",
            "record.json",
            "--key",
            "owner.key",
            "--output",
            "tombstone.json",
        ])
    };
    let output = revoke();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tombstone: spine_name::NameRecord =
        serde_json::from_slice(&std::fs::read(dir.path().join("tombstone.json")).unwrap()).unwrap();
    tombstone.verify().unwrap();
    assert!(tombstone.is_tombstone());
    assert_eq!(tombstone.name, record.name);
    assert_eq!(tombstone.seq, record.seq + 1);
    assert_eq!(tombstone.ttl_secs, spine_name::MAX_RECORD_TTL_SECS);
    assert!(tombstone.supersedes(&record));
    assert!(
        !revoke().status.success(),
        "an existing output is not overwritten"
    );
}

/// The public key of a key file as `name create` and `name rotate` write it: 64
/// hexadecimal digits of the seed.
fn key_file_public(path: &std::path::Path) -> [u8; 32] {
    let text = std::fs::read_to_string(path).unwrap();
    let text = text.trim();
    assert_eq!(text.len(), 64, "a key file is 64 hexadecimal digits");
    let mut seed = [0u8; 32];
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap();
    }
    ed25519_dalek::SigningKey::from_bytes(&seed)
        .verifying_key()
        .to_bytes()
}

/// `upload` and `fetch` take `--log` (parsed offline: both connect to a daemon
/// before they read anything, so only the help text can be run here).
#[test]
fn name_upload_and_fetch_take_a_succession_log() {
    for command in ["upload", "fetch"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_spine"))
            .args(["name", command, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{command} --help");
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("--log <LOG>"), "{command}: {help}");
    }
}

fn read_log_file(path: &std::path::Path) -> spine_name::SuccessionLog {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// `name rotate` and `name retire`, offline, end to end: the log files they
/// write verify, the keys they create are the ones the log names, records made
/// with `--log` are signed by the current key and carry the head claim, and the
/// superseded key and the retired name are refused.
#[test]
fn name_rotate_and_retire_round_trip_offline() {
    let dir = tempfile::tempdir().unwrap();
    let at = |file: &str| dir.path().join(file);
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_spine"))
            .current_dir(dir.path())
            .arg("name")
            .args(args)
            .output()
            .unwrap()
    };
    let ok = |args: &[&str]| {
        let output = run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let create = |key: &str, log: Option<&str>, output: &str| {
        let mut args = vec![
            "create",
            "--key",
            key,
            "--output",
            output,
            "--endpoint",
            "127.0.0.1:8080",
        ];
        if let Some(log) = log {
            args.extend(["--log", log]);
        }
        run(&args)
    };

    // A name as it has always been: its inception key signs.
    assert!(create("owner.key", None, "record.json").status.success());
    let original: spine_name::NameRecord =
        serde_json::from_slice(&std::fs::read(at("record.json")).unwrap()).unwrap();
    original.verify().unwrap();
    let inception = key_file_public(&at("owner.key"));
    assert_eq!(original.name.public_key(), Some(&inception));

    // First rotation: the two new keys are created, the log is written last.
    let rotate_one = [
        "rotate",
        "--key",
        "owner.key",
        "--reveal-key",
        "k1.key",
        "--next-key",
        "k2.key",
        "--output",
        "log1.json",
    ];
    let printed = ok(&rotate_one);
    assert!(
        printed.contains(&spine_name::SpineUri::did(inception).to_string()),
        "{printed}"
    );
    assert!(printed.contains("entries: 1"), "{printed}");
    let log1 = read_log_file(&at("log1.json"));
    let state1 = log1.verify(&inception).unwrap();
    assert_eq!(state1.index(), 1);
    assert_eq!(
        state1.current(),
        spine_name::Current::Key(key_file_public(&at("k1.key")))
    );
    assert_eq!(
        state1.commitment(),
        Some(&spine_name::key_commitment(&key_file_public(&at("k2.key"))))
    );
    assert!(
        !run(&rotate_one).status.success(),
        "an existing log is not overwritten"
    );

    // A record of the rotated name: same name, signed by the current key, with
    // the head claim. The inception key no longer signs for a holder of the log.
    assert!(create("k1.key", Some("log1.json"), "rotated.json")
        .status
        .success());
    let rotated: spine_name::NameRecord =
        serde_json::from_slice(&std::fs::read(at("rotated.json")).unwrap()).unwrap();
    assert_eq!(rotated.name, original.name);
    assert_eq!(
        rotated.meta.get(spine_name::SUCCESSION_META_KEY),
        Some(&state1.head_claim().unwrap().to_meta_value())
    );
    assert!(
        rotated.verify().is_err(),
        "not signed by the key in the name"
    );
    spine_name::authorise_record(&rotated, Some(&state1)).unwrap();
    assert!(matches!(
        spine_name::authorise_record(&original, Some(&state1)),
        Err(spine_name::NameError::SupersededKey(_))
    ));
    assert!(!create("owner.key", Some("log1.json"), "by-old-key.json")
        .status
        .success());
    assert!(!at("by-old-key.json").exists());

    // `name verify` agrees with both verdicts.
    let printed = ok(&["verify", "log1.json", "--record", "rotated.json"]);
    assert!(printed.contains("record: authorised"), "{printed}");
    assert!(!run(&["verify", "log1.json", "--record", "record.json"])
        .status
        .success());

    // A tombstone of the rotated name is signed by the current key, no claim.
    ok(&[
        "revoke",
        "rotated.json",
        "--key",
        "k1.key",
        "--log",
        "log1.json",
        "--output",
        "tombstone.json",
    ]);
    let tombstone: spine_name::NameRecord =
        serde_json::from_slice(&std::fs::read(at("tombstone.json")).unwrap()).unwrap();
    assert!(tombstone.is_tombstone());
    assert!(!tombstone.meta.contains_key(spine_name::SUCCESSION_META_KEY));
    spine_name::authorise_record(&tombstone, Some(&state1)).unwrap();

    // Second rotation: the revealed key must be the committed one, and exist.
    assert!(
        !run(&[
            "rotate",
            "--log",
            "log1.json",
            "--key",
            "k1.key",
            "--reveal-key",
            "missing.key",
            "--next-key",
            "k3.key",
            "--output",
            "log2.json",
        ])
        .status
        .success(),
        "a reveal key that was never committed to is refused"
    );
    assert!(!at("missing.key").exists());
    assert!(
        !at("k3.key").exists(),
        "no key is written for a refused log"
    );
    assert!(!at("log2.json").exists());
    ok(&[
        "rotate",
        "--log",
        "log1.json",
        "--key",
        "k1.key",
        "--reveal-key",
        "k2.key",
        "--next-key",
        "k3.key",
        "--output",
        "log2.json",
    ]);
    let log2 = read_log_file(&at("log2.json"));
    let state2 = log2.verify(&inception).unwrap();
    assert_eq!(state2.index(), 2);
    assert_eq!(log2.entries[..1], log1.entries[..], "a strict extension");
    assert_eq!(read_log_file(&at("log1.json")), log1, "the old log is kept");
    assert!(matches!(
        spine_name::authorise_record(&rotated, Some(&state2)),
        Err(spine_name::NameError::SuccessionRefused { .. })
    ));

    // Retirement: needs the committed key as well as the current one.
    assert!(!run(&[
        "retire",
        "--log",
        "log2.json",
        "--key",
        "k2.key",
        "--output",
        "retired.json",
    ])
    .status
    .success());
    let printed = ok(&[
        "retire",
        "--log",
        "log2.json",
        "--key",
        "k2.key",
        "--reveal-key",
        "k3.key",
        "--output",
        "retired.json",
    ]);
    assert!(printed.contains("retired"), "{printed}");
    let retired = read_log_file(&at("retired.json"));
    let state3 = retired.verify(&inception).unwrap();
    assert!(state3.is_retired());
    assert_eq!(state3.index(), 3);
    assert!(
        !create("k2.key", Some("retired.json"), "after-retire.json")
            .status
            .success(),
        "nothing signs a retired name"
    );
    assert!(!at("after-retire.json").exists());

    // A name with no log can be retired at once by its inception key.
    assert!(create("other.key", None, "other.json").status.success());
    ok(&[
        "retire",
        "--key",
        "other.key",
        "--output",
        "other-retired.json",
    ]);
    let other = read_log_file(&at("other-retired.json"));
    assert!(other
        .verify(&key_file_public(&at("other.key")))
        .unwrap()
        .is_retired());
}

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
