//! Acceptance test over separate core processes and real encrypted mesh sockets.
use ed25519_dalek::SigningKey;
use spine_agent::AgentClient;
use spine_name::{content_hash, resource_id, Endpoint, NameRecord, SpineUri};
use spine_protocol::ToolOutcome;
use std::{
    path::Path,
    process::{Child, Command, Stdio},
};
use tokio::net::TcpStream;

// Core startup and semantic encoding are CPU-intensive. Keep process tests
// independent of one another while still exercising real sockets/restarts.
static PROCESS_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn native_navigation_enforces_outbound_policy() {
    let _isolated = PROCESS_TEST.lock().await;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::time::timeout(std::time::Duration::from_secs(180), async {
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let allowed = format!("http://{}", http.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for reply in [
                &b"HTTP/1.1 200 OK\r\nContent-Length: 14\r\nConnection: close\r\n\r\n<h1>hello</h1>"[..],
                &b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"[..],
                &b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n"[..],
            ] {
                let (mut socket, _) = http.accept().await.unwrap();
                let mut request = [0; 4096];
                let _ = socket.read(&mut request).await;
                socket.write_all(reply).await.unwrap();
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let origin = origin_port();
        let mut config = spine_core::SpineConfig::default();
        config.server.host = "127.0.0.1".into();
        config.server.port = origin;
        config.server.metrics_port = port();
        let metrics_port = config.server.metrics_port;
        config.outbound.internal_origins = vec![allowed.clone()];
        config.outbound.max_response_bytes = 128;
        std::fs::write(dir.path().join("spine.toml"), toml::to_string(&config).unwrap()).unwrap();
        let mut node = launch(dir.path());
        let mut client = connect(&mut node, dir.path(), origin).await;
        assert!(client.navigate("http://127.0.0.1:1/").await.unwrap_err().to_string().contains("outbound_address_denied"));
        client.navigate(&allowed).await.unwrap();
        assert_eq!(client.get_raw_html().await.unwrap(), "<h1>hello</h1>");
        assert!(client.navigate(&allowed).await.unwrap_err().to_string().contains("outbound_redirect_denied"));
        assert!(client.navigate(&allowed).await.unwrap_err().to_string().contains("outbound_response_limit"));
        assert_eq!(client.get_raw_html().await.unwrap(), "<h1>hello</h1>");
        server.await.unwrap();
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let health: serde_json::Value = http.get(format!("http://127.0.0.1:{metrics_port}/health")).send().await.unwrap().json().await.unwrap();
        assert_eq!(health["outbound"]["started"], 4);
        assert_eq!(health["outbound"]["succeeded"], 1);
        assert_eq!(health["outbound"]["failed"], 3);
        assert_eq!(health["outbound"]["denied"], 2);
        assert_eq!(health["outbound"]["response_limit_failures"], 1);
        assert_eq!(health["outbound"]["active"], 0);
        let text = http.get(format!("http://127.0.0.1:{metrics_port}/metrics")).send().await.unwrap().text().await.unwrap();
        assert!(text.contains("spine_outbound_calls_started_total 4\n"));
        assert!(text.contains("spine_outbound_calls_denied_total 2\n"));
        assert!(!text.contains(&allowed));
    }).await.expect("native outbound acceptance deadline exceeded");
}

struct Node(Child);
impl Drop for Node {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn port() -> u16 {
    static USED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<u16>>> =
        std::sync::OnceLock::new();
    let mut used = USED.get_or_init(Default::default).lock().unwrap();
    loop {
        let p = 20000 + rand::random::<u16>() % 30000;
        if [p, p + 1, p + 1000].iter().any(|v| used.contains(v)) {
            continue;
        }
        if let (Ok(_a), Ok(_b), Ok(_c)) = (
            std::net::TcpListener::bind(("127.0.0.1", p)),
            std::net::TcpListener::bind(("127.0.0.1", p + 1)),
            std::net::TcpListener::bind(("127.0.0.1", p + 1000)),
        ) {
            used.extend([p, p + 1, p + 1000]);
            return p;
        }
    }
}

fn origin_port() -> u16 {
    loop {
        let p = port();
        if p > 64000 {
            continue;
        }
        if let (Ok(_ws), Ok(_cluster)) = (
            std::net::TcpListener::bind(("127.0.0.1", p + 1)),
            std::net::TcpListener::bind(("127.0.0.1", p + 1000)),
        ) {
            return p;
        }
    }
}

fn start(dir: &Path, origin: u16, mesh: u16, seeds: &[String]) -> Node {
    start_with_tasks(dir, origin, mesh, seeds, &[])
}

fn start_with_tasks(
    dir: &Path,
    origin: u16,
    mesh: u16,
    seeds: &[String],
    allowed: &[String],
) -> Node {
    let config = format!("[server]\nhost = \"127.0.0.1\"\nport = {origin}\nmetrics_port = {}\n[namespace]\nenabled = true\nport = {mesh}\nseeds = {}\nmaintain_secs = 1\ntasks_enabled = {}\ntask_peers = {}\n", port(), serde_json::to_string(seeds).unwrap(),!allowed.is_empty(),serde_json::to_string(allowed).unwrap());
    std::fs::write(dir.join("spine.toml"), config).unwrap();
    launch(dir)
}

fn launch(dir: &Path) -> Node {
    let log = std::fs::File::create(dir.join("server.log")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_spine-core"));
    command
        .current_dir(dir)
        .stdin(Stdio::piped())
        .env("SPINE_MANAGED_CHILD", "1")
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    for key in [
        "PORT",
        "SPINE_PORT",
        "SPINE_HOST",
        "SPINE_METRICS_PORT",
        "SPINE_SEEDS",
        "SPINE_TLS",
        "SPINE_CONFIG_PATH",
    ] {
        command.env_remove(key);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    Node(command.spawn().unwrap())
}

#[tokio::test]
async fn managed_shutdown_drains_and_keeps_published_bytes() {
    let _isolated = PROCESS_TEST.lock().await;
    tokio::time::timeout(std::time::Duration::from_secs(180), async {
        let dir = tempfile::tempdir().unwrap();
        let origin_port = origin_port();
        let mesh_port = port();
        eprintln!("managed acceptance: starting origin");
        let mut node = start(dir.path(), origin_port, mesh_port, &[]);
        let mut client = connect(&mut node, dir.path(), origin_port).await;
        let key = SigningKey::from_bytes(&[44; 32]);
        let mut record = NameRecord::new(
            SpineUri::did(key.verifying_key().to_bytes()),
            1,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        )
        .unwrap()
        .with_content_hash(resource_id("text/plain", b"graceful bytes").unwrap());
        record.sign(&key).unwrap();
        client
            .publish_resource(&record, "text/plain", b"graceful bytes".to_vec())
            .await
            .unwrap();
        eprintln!("managed acceptance: publication acknowledged; requesting drain");
        drop(client);
        use std::io::Write;
        node.0.stdin.take().unwrap().write_all(&[1]).unwrap();
        loop {
            if let Some(status) = node.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let log = std::fs::read_to_string(dir.path().join("server.log")).unwrap();
        assert!(log.contains("All connections drained"));
        eprintln!("managed acceptance: drained; restarting origin");
        node = start(dir.path(), origin_port, mesh_port, &[]);
        let mut client = connect(&mut node, dir.path(), origin_port).await;
        assert_eq!(
            client.read_resource(record).await.unwrap().data,
            b"graceful bytes"
        );
    })
    .await
    .expect("managed shutdown acceptance deadline exceeded");
}

#[tokio::test]
async fn signed_mesh_tasks_dependencies_knowledge_restart_and_authorization() {
    let _isolated = PROCESS_TEST.lock().await;
    use spine_protocol::tasks::{TaskInput, TaskOperation, TaskRecord, TaskSpec, TaskState};
    tokio::time::timeout(std::time::Duration::from_secs(180), async {
        let caller_dir = tempfile::tempdir().unwrap();
        let worker_dir = tempfile::tempdir().unwrap();
        let outsider_dir = tempfile::tempdir().unwrap();
        for (dir, seed) in [
            (&caller_dir, 21u8),
            (&worker_dir, 22u8),
            (&outsider_dir, 23u8),
        ] {
            std::fs::write(
                dir.path().join("spine-node.key"),
                format!("{seed:02x}").repeat(32),
            )
            .unwrap();
        }
        let caller_key = SigningKey::from_bytes(&[21; 32]).verifying_key().to_bytes();
        let worker_key = SigningKey::from_bytes(&[22; 32]).verifying_key().to_bytes();
        let allowed = vec![caller_key
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()];
        let (worker_port, worker_mesh, caller_port, caller_mesh) =
            (origin_port(), port(), origin_port(), port());
        let mut worker =
            start_with_tasks(worker_dir.path(), worker_port, worker_mesh, &[], &allowed);
        let mut origin = connect(&mut worker, worker_dir.path(), worker_port).await;
        let status = origin.namespace_status().await.unwrap();
        let worker_id = status["agent_id"].as_str().unwrap().to_owned();
        let mut caller_node = start(
            caller_dir.path(),
            caller_port,
            caller_mesh,
            &[format!("127.0.0.1:{worker_mesh}")],
        );
        let mut caller = connect(&mut caller_node, caller_dir.path(), caller_port).await;
        let metadata = caller
            .mesh_call(
                &worker_id,
                worker_key,
                serde_json::to_value(TaskOperation::Capabilities).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(metadata["version"], 1);
        assert_eq!(metadata["tools"].as_array().unwrap().len(), 2);
        let first = TaskSpec {
            id: uuid::Uuid::new_v4(),
            tool: "spine.content/hash".into(),
            args: serde_json::json!({"text":"artifact source"}),
            dependencies: vec![],
            input_from: None,
            timeout_secs: 300,
        };
        let second = TaskSpec {
            id: uuid::Uuid::new_v4(),
            tool: "spine.content/hash".into(),
            args: serde_json::json!({"text":""}),
            dependencies: vec![first.id],
            input_from: Some(TaskInput {
                task_id: first.id,
                pointer: "/sha256".into(),
            }),
            timeout_secs: 300,
        };
        let submit = serde_json::to_value(TaskOperation::Submit {
            spec: first.clone(),
        })
        .unwrap();
        assert!(
            caller
                .mesh_call(&worker_id, [0; 32], submit.clone())
                .await
                .is_err(),
            "mismatched pin must refuse delivery"
        );
        caller
            .mesh_call(&worker_id, worker_key, submit.clone())
            .await
            .unwrap();
        caller
            .mesh_call(&worker_id, worker_key, submit.clone())
            .await
            .unwrap();
        caller
            .mesh_call(
                &worker_id,
                worker_key,
                serde_json::to_value(TaskOperation::Submit {
                    spec: second.clone(),
                })
                .unwrap(),
            )
            .await
            .unwrap();
        let record = loop {
            let value = caller
                .mesh_call(
                    &worker_id,
                    worker_key,
                    serde_json::to_value(TaskOperation::Status { id: second.id }).unwrap(),
                )
                .await
                .unwrap();
            let record: TaskRecord = serde_json::from_value(value).unwrap();
            if record.state == TaskState::Completed {
                break record;
            }
            assert_eq!(record.error, None);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        };
        let ToolOutcome::Ok { content } = record.result.unwrap().outcome else {
            panic!("dependent task failed")
        };
        let first_hex: String = content_hash(b"artifact source")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let second_hex: String = content_hash(first_hex.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(content["sha256"], second_hex);
        caller
            .mesh_call(
                &worker_id,
                worker_key,
                serde_json::to_value(TaskOperation::KnowledgePut {
                    key: "artifact".into(),
                    value: content.clone(),
                })
                .unwrap(),
            )
            .await
            .unwrap();
        let outsider_port = origin_port();
        let mut outsider_node = start(
            outsider_dir.path(),
            outsider_port,
            port(),
            &[format!("127.0.0.1:{worker_mesh}")],
        );
        let mut outsider = connect(&mut outsider_node, outsider_dir.path(), outsider_port).await;
        assert!(outsider
            .mesh_call(
                &worker_id,
                worker_key,
                serde_json::to_value(TaskOperation::Status { id: first.id }).unwrap()
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("not_authorized"));
        drop(origin);
        drop(worker);
        worker = start_with_tasks(
            worker_dir.path(),
            worker_port,
            worker_mesh,
            &[format!("127.0.0.1:{caller_mesh}")],
            &allowed,
        );
        origin = connect(&mut worker, worker_dir.path(), worker_port).await;
        assert_eq!(
            origin.namespace_status().await.unwrap()["agent_id"],
            worker_id
        );
        let restored = caller
            .mesh_call(
                &worker_id,
                worker_key,
                serde_json::to_value(TaskOperation::Status { id: second.id }).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(restored["state"], "Completed");
        assert_eq!(
            caller
                .mesh_call(
                    &worker_id,
                    worker_key,
                    serde_json::to_value(TaskOperation::KnowledgeGet {
                        key: "artifact".into()
                    })
                    .unwrap()
                )
                .await
                .unwrap(),
            content
        );
        caller
            .mesh_call(&worker_id, worker_key, submit)
            .await
            .unwrap();
        let mut conflict = first;
        conflict.args = serde_json::json!({"text":"conflicting mutation"});
        assert!(caller
            .mesh_call(
                &worker_id,
                worker_key,
                serde_json::to_value(TaskOperation::Submit { spec: conflict }).unwrap()
            )
            .await
            .is_err());
    })
    .await
    .expect("signed coordination acceptance deadline exceeded");
}

async fn connect(node: &mut Node, dir: &Path, port: u16) -> AgentClient<TcpStream> {
    // A debug-build daemon on a loaded CI runner can take well over ten seconds
    // to start listening; seven of these tests start daemons at once.
    for _ in 0..600 {
        if let Some(status) = node.0.try_wait().unwrap() {
            panic!(
                "core exited {status}: {}",
                std::fs::read_to_string(dir.join("server.log")).unwrap()
            );
        }
        if let Ok(Ok(client)) = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            AgentClient::<TcpStream>::connect(&format!("127.0.0.1:{port}")),
        )
        .await
        {
            return client;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!(
        "core did not become ready: {}",
        std::fs::read_to_string(dir.join("server.log")).unwrap()
    );
}

#[tokio::test]
async fn discover_fetch_restart_and_renew_over_independent_nodes() {
    let _isolated = PROCESS_TEST.lock().await;
    tokio::time::timeout(std::time::Duration::from_secs(300), async {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let (first_port, first_mesh, second_port, second_mesh) =
            (origin_port(), port(), origin_port(), port());
        eprintln!("resource acceptance: starting two nodes");
        let mut first = start(first_dir.path(), first_port, first_mesh, &[]);
        let mut origin = connect(&mut first, first_dir.path(), first_port).await;
        let mut second = start(
            second_dir.path(),
            second_port,
            second_mesh,
            &[format!("127.0.0.1:{first_mesh}")],
        );
        let mut agent = connect(&mut second, second_dir.path(), second_port).await;
        assert_eq!(agent.capabilities().await.unwrap().capabilities.len(), 2);
        assert!(agent
            .send_speech_act(uuid::Uuid::new_v4(), "inform", "private speech")
            .await
            .unwrap_err()
            .to_string()
            .contains("unsupported_speech_act"));
        assert!(agent
            .delegate_task("private task", None)
            .await
            .unwrap_err()
            .to_string()
            .contains("unsupported_legacy_swarm"));
        assert!(agent.create_swarm_plan("private goal").await.is_err());
        assert!(agent
            .execute_plan_task(uuid::Uuid::new_v4(), uuid::Uuid::new_v4())
            .await
            .is_err());
        eprintln!("resource acceptance: native operations connected");
        let parsed = agent
            .call_tool(
                "spine.html/parse",
                serde_json::json!({"text":"<p>independent agent</p>"}),
            )
            .await
            .unwrap();
        assert!(matches!(
            parsed.outcome,
            spine_protocol::ToolOutcome::Ok { .. }
        ));
        let rejected = agent
            .call_tool("shell", serde_json::json!({"text":"test"}))
            .await
            .unwrap();
        assert!(matches!(
            rejected.outcome,
            spine_protocol::ToolOutcome::Err { .. }
        ));
        let key = SigningKey::from_bytes(&[77; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes()).with_path("/documents/example");
        let bytes = vec![37; 150_000];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut record = NameRecord::new(name.clone(), 1, now)
            .unwrap()
            .with_capability("test.documents")
            .with_endpoint(Endpoint::new("tcp", format!("127.0.0.1:{first_port}")))
            .with_content_hash(resource_id("application/octet-stream", &bytes).unwrap());
        record.sign(&key).unwrap();
        origin
            .publish_resource(&record, "application/octet-stream", bytes.clone())
            .await
            .unwrap();
        let providers = agent.find_providers("test.documents", None).await.unwrap();
        assert_eq!(
            providers["count"],
            1,
            "first: {}\nsecond: {}",
            std::fs::read_to_string(first_dir.path().join("server.log")).unwrap(),
            std::fs::read_to_string(second_dir.path().join("server.log")).unwrap()
        );
        assert_eq!(
            agent
                .fetch_resource_with(
                    &name.to_string(),
                    spine_agent::resources::FetchPolicy::allow_private(),
                )
                .await
                .unwrap()
                .data,
            bytes
        );
        eprintln!("resource acceptance: remote resource fetched; restarting publisher");
        drop(origin);
        drop(first);
        first = start(
            first_dir.path(),
            first_port,
            first_mesh,
            &[format!("127.0.0.1:{second_mesh}")],
        );
        origin = connect(&mut first, first_dir.path(), first_port).await;
        assert_eq!(
            origin.read_resource(record.clone()).await.unwrap().data,
            bytes
        );
        eprintln!("resource acceptance: publisher resource restored; renewing record");
        let renewed = record.renewed(&key, now + 1, 3600).unwrap();
        origin
            .publish_name(serde_json::to_value(&renewed).unwrap())
            .await
            .unwrap();
        let resolved = origin.resolve_name(&name.to_string(), None).await.unwrap();
        let spine_protocol::NameResolution::Resolved {
            record: restored, ..
        } = resolved
        else {
            panic!("renewed name did not resolve");
        };
        assert_eq!(restored["seq"], 2);
        assert_eq!(
            agent
                .fetch_resource_with(
                    &name.to_string(),
                    spine_agent::resources::FetchPolicy::allow_private(),
                )
                .await
                .unwrap()
                .data,
            bytes
        );
        let unrelated = NameRecord::new(
            SpineUri::did(SigningKey::from_bytes(&[99; 32]).verifying_key().to_bytes()),
            1,
            now,
        )
        .unwrap();
        assert!(origin
            .publish_resource(&unrelated, "text/plain", vec![1])
            .await
            .is_err());
    })
    .await
    .expect("agent-web acceptance test exceeded its deadline");
}

#[tokio::test]
async fn native_idle_connections_release_global_admission() {
    let _isolated = PROCESS_TEST.lock().await;
    use std::time::Duration;
    tokio::time::timeout(Duration::from_secs(180), async {
        let dir = tempfile::tempdir().unwrap();
        let origin = origin_port();
        let metrics_port = port();
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let config = format!("[server]\nhost = \"127.0.0.1\"\nport = {origin}\nmetrics_port = {metrics_port}\nmax_connections = 1\nmax_connections_per_ip = 1\nidle_timeout_secs = 6\n");
        std::fs::write(dir.path().join("spine.toml"), config).unwrap();
        let mut node = launch(dir.path());
        let mut first = connect(&mut node, dir.path(), origin).await;
        assert_eq!(first.capabilities().await.unwrap().capabilities.len(), 2);
        let response = http.get(format!("http://127.0.0.1:{metrics_port}/ready")).send().await.unwrap();
        assert_eq!(response.status().as_u16(), 503);
        let readiness: serde_json::Value = response.json().await.unwrap();
        assert_eq!(readiness["reasons"], serde_json::json!(["connection_capacity"]));
        assert_eq!(readiness["active_connections"], 1);
        assert_eq!(readiness["namespace"], "disabled");
        assert_eq!(http.get(format!("http://127.0.0.1:{metrics_port}/health")).send().await.unwrap().status().as_u16(), 200);
        first.ping().await.unwrap();
        let mut excess = AgentClient::<TcpStream>::connect(&format!("127.0.0.1:{origin}")).await.unwrap();
        assert!(excess.capabilities().await.is_err());
        drop(excess);
        let text = http.get(format!("http://127.0.0.1:{metrics_port}/metrics")).send().await.unwrap().text().await.unwrap();
        assert!(text.contains("spine_native_connections_rejected_global_total 1\n"));
        assert!(text.contains("spine_native_connections_active 1\n"));
        // No further traffic from first. Its complete-message idle deadline
        // releases the shared connection budget even if the client stays open.
        tokio::time::sleep(Duration::from_millis(6500)).await;
        let response = http.get(format!("http://127.0.0.1:{metrics_port}/ready")).send().await.unwrap();
        assert_eq!(response.status().as_u16(), 200);
        let readiness: serde_json::Value = response.json().await.unwrap();
        assert_eq!(readiness["active_connections"], 0);
        assert_eq!(readiness["reasons"], serde_json::json!([]));
        let mut next = AgentClient::<TcpStream>::connect(&format!("127.0.0.1:{origin}")).await.unwrap();
        assert_eq!(next.capabilities().await.unwrap().capabilities.len(), 2);
    }).await.expect("native admission acceptance deadline exceeded");
}

#[tokio::test]
async fn configured_mutual_tls_protects_tcp_and_websocket_origins() {
    let _isolated = PROCESS_TEST.lock().await;
    use std::{sync::Arc, time::Duration};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::{
        rustls::{pki_types::ServerName, ClientConfig, RootCertStore},
        TlsConnector,
    };
    tokio::time::timeout(Duration::from_secs(30), async {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let dir = tempfile::tempdir().unwrap();
        let certs = dir.path().join("certs");
        spine_core::tls::generate_dev_certs(&certs).unwrap();
        let origin = origin_port();
        let config = format!("[server]\nhost = \"127.0.0.1\"\nport = {origin}\nmetrics_port = {}\n[tls]\nenabled = true\nmutual_tls = true\n", port());
        std::fs::write(dir.path().join("spine.toml"), config).unwrap();
        let mut node = launch(dir.path());
        drop(connect(&mut node, dir.path(), origin).await);
        let mut roots = RootCertStore::empty();
        for cert in spine_core::tls::load_certs(&certs.join("ca.pem")).unwrap() { roots.add(cert).unwrap(); }
        let anonymous = TlsConnector::from(Arc::new(ClientConfig::builder().with_root_certificates(roots.clone()).with_no_client_auth()));
        let authorized = TlsConnector::from(Arc::new(ClientConfig::builder().with_root_certificates(roots)
            .with_client_auth_cert(spine_core::tls::load_certs(&certs.join("client-cert.pem")).unwrap(),
                spine_core::tls::load_private_key(&certs.join("client-key.pem")).unwrap()).unwrap()));
        let mut agent = AgentClient::<tokio_rustls::client::TlsStream<TcpStream>>::connect_tls(
            &format!("127.0.0.1:{origin}"), "localhost", Some(&certs.join("ca.pem")),
            Some((&certs.join("client-cert.pem"), &certs.join("client-key.pem"))),
        ).await.unwrap_or_else(|error| panic!("TLS origin failed: {error}; {}",
            std::fs::read_to_string(dir.path().join("server.log")).unwrap()));
        assert_eq!(agent.capabilities().await.unwrap().capabilities.len(), 2);
        // Morphing the framing must leave the connection usable: both ends switch
        // after the response, so the next exchange succeeds.
        agent.morph().await.unwrap();
        agent.ping().await.unwrap();
        drop(agent);
        // A TLS 1.3 connector may finish its local handshake before the server
        // rejects its missing certificate; a successful application reply is
        // the authorization criterion.
        if let Ok(tls) = anonymous.connect(ServerName::try_from("localhost").unwrap(), TcpStream::connect(("127.0.0.1", origin)).await.unwrap()).await {
            let mut handler = spine_protocol::ProtocolHandler::new(tls);
            let _ = handler.send_message(&spine_protocol::Message::Ping { timestamp: 58 }).await;
            assert!(handler.receive_message().await.is_err());
        }
        let upgrade = b"GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n";
        let mut tls = authorized.connect(ServerName::try_from("localhost").unwrap(), TcpStream::connect(("127.0.0.1", origin + 1)).await.unwrap()).await.unwrap();
        tls.write_all(upgrade).await.unwrap();
        let mut response = Vec::new();
        while !response.ends_with(b"\r\n\r\n") { response.push(tls.read_u8().await.unwrap()); assert!(response.len() <= 4096); }
        assert!(response.starts_with(b"HTTP/1.1 101"));
        drop(tls);
        if let Ok(mut tls) = anonymous.connect(ServerName::try_from("localhost").unwrap(), TcpStream::connect(("127.0.0.1", origin + 1)).await.unwrap()).await {
            let _ = tls.write_all(upgrade).await;
            let mut response = [0; 256];
            let read = tls.read(&mut response).await;
            assert!(read.is_err() || matches!(read, Ok(0)), "missing client identity cannot upgrade WSS");
        }
        let mut plain = TcpStream::connect(("127.0.0.1", origin + 1)).await.unwrap();
        plain.write_all(upgrade).await.unwrap();
        let mut response = [0; 256];
        let read = plain.read(&mut response).await;
        assert!(read.is_err() || !response.starts_with(b"HTTP/1.1 101"), "TLS-enabled origin cannot accept a plaintext WebSocket upgrade");
    }).await.expect("mutual TLS origin acceptance deadline exceeded");
}

#[tokio::test]
async fn detached_session_retention_recovers_capacity_and_does_not_resurrect() {
    let _isolated = PROCESS_TEST.lock().await;
    tokio::time::timeout(std::time::Duration::from_secs(180), async {
        let dir = tempfile::tempdir().unwrap();
        let origin = origin_port();
        let mut config = spine_core::SpineConfig::default();
        config.server.host = "127.0.0.1".into();
        config.server.port = origin;
        config.server.metrics_port = port();
        config.server.max_sessions = 1;
        config.server.session_retention_secs = 2;
        config.server.session_watchdog_secs = 1;
        config.server.persistence_interval_secs = 1;
        config.server.idle_timeout_secs = 60;
        std::fs::write(dir.path().join("spine.toml"), toml::to_string(&config).unwrap()).unwrap();
        let manifest = dir.path().join("spine-sessions-v1.json");
        let mut node = launch(dir.path());
        let mut first = connect(&mut node, dir.path(), origin).await;
        let mut second = connect(&mut node, dir.path(), origin).await;
        let (a, b) = tokio::join!(first.get_raw_html(), second.get_raw_html());
        let a_busy = a.unwrap_err().to_string().contains("maximum sessions");
        let b_busy = b.unwrap_err().to_string().contains("maximum sessions");
        assert_ne!(a_busy, b_busy, "exactly one racing client must obtain the last session slot");
        let mut live = if a_busy { drop(first); second } else { drop(second); first };
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        // Connected sessions remain usable even after the retention interval.
        assert!(!live.get_raw_html().await.unwrap_err().to_string().contains("expired"));
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let ready = format!("http://127.0.0.1:{}/ready", config.server.metrics_port);
        let pressure = http.get(&ready).send().await.unwrap();
        assert_eq!(pressure.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        let pressure: serde_json::Value = pressure.json().await.unwrap();
        assert_eq!(pressure["sessions"], 1);
        assert!(pressure["reasons"].as_array().unwrap().iter().any(|v| v == "session_capacity"));
        let original: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        let (id, session) = original["sessions"].as_object().unwrap().iter().next().unwrap();
        let id = id.clone();
        let mut legacy = session.clone();
        legacy.as_object_mut().unwrap().remove("retained_at");
        drop(live);
        // Allow expiry plus the next watchdog tick, without depending on precise scheduler timing.
        for _ in 0..100 {
            let response = http.get(&ready).send().await.unwrap();
            if response.status().is_success() { break; }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let recovered: serde_json::Value = http.get(&ready).send().await.unwrap().json().await.unwrap();
        assert_eq!(recovered["sessions"], 0);
        let mut replacement = connect(&mut node, dir.path(), origin).await;
        assert!(!replacement.get_raw_html().await.unwrap_err().to_string().contains("maximum sessions"));
        drop(replacement);
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        drop(node);
        let expired: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        assert!(expired["sessions"].as_object().unwrap().is_empty());
        // A stale pre-migration file must never override the authoritative empty snapshot.
        std::fs::create_dir(dir.path().join("sessions")).unwrap();
        std::fs::write(dir.path().join("sessions").join(format!("{id}.json")), serde_json::to_vec(&legacy).unwrap()).unwrap();
        node = launch(dir.path());
        let probe = connect(&mut node, dir.path(), origin).await;
        let restored: serde_json::Value = http.get(&ready).send().await.unwrap().json().await.unwrap();
        assert_eq!(restored["sessions"], 0);
        drop(probe);
        drop(node);
        // Startup also expires a detached record saved before the watchdog had time to reap it.
        legacy["retained_at"] = serde_json::json!(1);
        let snapshot = serde_json::json!({"version":1,"sessions":{id.clone():legacy},"knowledge":{id:[],"cluster_shared":[]}});
        std::fs::write(&manifest, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        node = launch(dir.path());
        let _probe = connect(&mut node, dir.path(), origin).await;
        let restored: serde_json::Value = http.get(&ready).send().await.unwrap().json().await.unwrap();
        assert_eq!(restored["sessions"], 0);
        let stored: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        assert!(stored["sessions"].as_object().unwrap().is_empty());
        assert_eq!(stored["knowledge"].as_object().unwrap().len(), 1);
        assert!(stored["knowledge"].get("cluster_shared").is_some());
    }).await.expect("session retention acceptance deadline exceeded");
}
