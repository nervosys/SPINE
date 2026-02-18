#!/usr/bin/env python3
"""Patch spine-gateway/src/main.rs to add /metrics endpoint, route, and tests."""
import sys

f = r"c:\Users\adamm\dev\nervosys\web\Hyperlight\spine-gateway\src\main.rs"
with open(f, "r", encoding="utf-8") as fh:
    c = fh.read()

# 1. Add /metrics route after /ready route
old_route = '        .route("/ready", get(ready))\n'
new_route = '        .route("/ready", get(ready))\n        .route("/metrics", get(metrics))\n'
if '/metrics' not in c:
    c = c.replace(old_route, new_route)
    print("1. Added /metrics route")
else:
    print("1. /metrics route already exists")

# 2. Add metrics endpoint function after ready()
metrics_fn = '''
/// Prometheus metrics
async fn metrics(State(state): State<Arc<AppState>>) -> Response {
    let uptime = state.start_time.elapsed().as_secs();
    let sessions = state.sessions.len() as u64;
    let requests = state.requests_total.load(Ordering::Relaxed);
    let errors = state.errors_total.load(Ordering::Relaxed);

    let body = format!(
        "# HELP spine_gateway_uptime_seconds Gateway uptime in seconds\\n\\
         # TYPE spine_gateway_uptime_seconds gauge\\n\\
         spine_gateway_uptime_seconds {uptime}\\n\\
         # HELP spine_gateway_sessions_active Number of active gateway sessions\\n\\
         # TYPE spine_gateway_sessions_active gauge\\n\\
         spine_gateway_sessions_active {sessions}\\n\\
         # HELP spine_gateway_requests_total Total requests processed\\n\\
         # TYPE spine_gateway_requests_total counter\\n\\
         spine_gateway_requests_total {requests}\\n\\
         # HELP spine_gateway_errors_total Total errors\\n\\
         # TYPE spine_gateway_errors_total counter\\n\\
         spine_gateway_errors_total {errors}\\n\\
         # HELP spine_gateway_max_sessions Maximum sessions allowed\\n\\
         # TYPE spine_gateway_max_sessions gauge\\n\\
         spine_gateway_max_sessions {}\\n",
        state.max_sessions
    );

    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
        .into_response()
}

'''

if 'async fn metrics(' not in c:
    marker = '// ---------------------------------------------------------------------------\n// Main\n// ---------------------------------------------------------------------------'
    c = c.replace(marker, metrics_fn + marker)
    print("2. Added metrics endpoint function")
else:
    print("2. metrics endpoint already exists") 

# 3. Add tests
test_code = '''
    #[test]
    fn test_app_state_counters() {
        let config = SpineConfig::load();
        let state = AppState::new(&config);
        assert_eq!(state.requests_total.load(Ordering::Relaxed), 0);
        assert_eq!(state.errors_total.load(Ordering::Relaxed), 0);
        state.requests_total.fetch_add(5, Ordering::Relaxed);
        state.errors_total.fetch_add(2, Ordering::Relaxed);
        assert_eq!(state.requests_total.load(Ordering::Relaxed), 5);
        assert_eq!(state.errors_total.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let config = SpineConfig::load();
        let state = Arc::new(AppState::new(&config));
        state.requests_total.fetch_add(42, Ordering::Relaxed);
        state.errors_total.fetch_add(3, Ordering::Relaxed);

        let resp = metrics(State(state)).await;
        let body = axum::body::to_bytes(resp.into_body(), 10_000)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();

        assert!(text.contains("spine_gateway_requests_total 42"));
        assert!(text.contains("spine_gateway_errors_total 3"));
        assert!(text.contains("spine_gateway_sessions_active 0"));
        assert!(text.contains("spine_gateway_uptime_seconds"));
    }
'''

if 'test_app_state_counters' not in c:
    # Insert before final closing brace of tests module
    # Find last }
    last_brace = c.rfind('}')
    c = c[:last_brace] + test_code + '\n}\n'
    print("3. Added tests")
else:
    print("3. Tests already exist")

# 4. Add #[instrument(skip_all)] to create_session
if '#[instrument' not in c:
    c = c.replace(
        'async fn create_session(\n',
        '#[instrument(skip_all)]\nasync fn create_session(\n'
    )
    print("4. Added #[instrument] to create_session")
else:
    print("4. #[instrument] already added")

with open(f, "w", encoding="utf-8", newline='\n') as fh:
    fh.write(c)

print("Done. File written.")
