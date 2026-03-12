"""Apply remaining Phase 18 edits: metrics endpoint + agent instrumentation + tests."""
import re

# ============================================================
# 1. Gateway: add metrics() endpoint function before main()
# ============================================================
gw = "src/spine-gateway/src/main.rs"
with open(gw, "r", encoding="utf-8") as f:
    src = f.read()

# Insert metrics function before the Main section separator
metrics_fn = '''/// Prometheus-compatible metrics endpoint
async fn metrics(State(state): State<Arc<AppState>>) -> Response {
    let uptime = state.start_time.elapsed().as_secs();
    let sessions = state.sessions.len();
    let requests = state.requests_total.load(Ordering::Relaxed);
    let errors = state.errors_total.load(Ordering::Relaxed);

    let body = format!(
        "# HELP spine_gateway_uptime_seconds Gateway uptime in seconds\\n\\
         # TYPE spine_gateway_uptime_seconds gauge\\n\\
         spine_gateway_uptime_seconds {}\\n\\
         # HELP spine_gateway_active_sessions Number of active sessions\\n\\
         # TYPE spine_gateway_active_sessions gauge\\n\\
         spine_gateway_active_sessions {}\\n\\
         # HELP spine_gateway_requests_total Total API requests\\n\\
         # TYPE spine_gateway_requests_total counter\\n\\
         spine_gateway_requests_total {}\\n\\
         # HELP spine_gateway_errors_total Total API errors\\n\\
         # TYPE spine_gateway_errors_total counter\\n\\
         spine_gateway_errors_total {}\\n",
        uptime, sessions, requests, errors
    );

    Response::builder()
        .header("content-type", "text/plain; version=0.0.4")
        .body(body.into())
        .unwrap()
}

'''

marker = "// ---------------------------------------------------------------------------\n// Main\n"
if "async fn metrics" not in src:
    if marker in src:
        src = src.replace(marker, metrics_fn + marker)
        print("  [OK] Added metrics() endpoint to gateway")
    else:
        print("  [WARN] Could not find Main section marker in gateway")
else:
    print("  [SKIP] metrics() endpoint already exists")

# 2. Add gateway tests for metrics/observability
test_code = '''
    #[test]
    fn test_app_state_counters() {
        let config = SpineConfig::default();
        let state = AppState::new(&config);
        assert_eq!(state.requests_total.load(Ordering::Relaxed), 0);
        assert_eq!(state.errors_total.load(Ordering::Relaxed), 0);
        state.requests_total.fetch_add(5, Ordering::Relaxed);
        state.errors_total.fetch_add(2, Ordering::Relaxed);
        assert_eq!(state.requests_total.load(Ordering::Relaxed), 5);
        assert_eq!(state.errors_total.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_grafana_dashboard_exists() {
        assert!(std::path::Path::new("deploy/grafana/spine-dashboard.json").exists()
            || std::path::Path::new("../deploy/grafana/spine-dashboard.json").exists());
    }
'''

if "test_app_state_counters" not in src:
    # Insert before the final closing brace of the tests module
    # Find the last "}" in the file
    last_brace = src.rfind("}")
    if last_brace > 0:
        src = src[:last_brace] + test_code + "\n}\n"
        print("  [OK] Added gateway observability tests")
else:
    print("  [SKIP] Gateway tests already exist")

with open(gw, "w", encoding="utf-8", newline="\n") as f:
    f.write(src)

# ============================================================
# 3. Agent: add #[instrument] to key methods
# ============================================================
agent = "src/spine-agent/src/lib.rs"
with open(agent, "r", encoding="utf-8") as f:
    src = f.read()

# Add `use tracing::instrument;` if not present
if "use tracing::instrument" not in src:
    # Add after the last `use` statement before the struct definition
    src = src.replace(
        "use spine_protocol::",
        "use tracing::instrument;\nuse spine_protocol::",
        1,
    )
    print("  [OK] Added tracing::instrument import to agent")

# Add #[instrument] before key methods
instrument_targets = [
    ("    pub async fn navigate(", '    #[instrument(skip(self))]\n    pub async fn navigate('),
    ("    pub async fn get_ur(", '    #[instrument(skip(self))]\n    pub async fn get_ur('),
    ("    pub async fn search(", '    #[instrument(skip(self))]\n    pub async fn search('),
    ("    pub async fn ping(", '    #[instrument(skip(self))]\n    pub async fn ping('),
    ("    pub async fn execute_hls(", '    #[instrument(skip(self))]\n    pub async fn execute_hls('),
]

for old, new in instrument_targets:
    if old in src and f"#[instrument" not in src.split(old)[0].split("\n")[-2]:
        src = src.replace(old, new, 1)
        method = old.split("fn ")[1].split("(")[0]
        print(f"  [OK] Added #[instrument] to agent::{method}")

with open(agent, "w", encoding="utf-8", newline="\n") as f:
    f.write(src)

print("\nAll Phase 18 edits applied successfully.")
