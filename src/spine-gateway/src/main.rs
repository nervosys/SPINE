//! # SPINE Gateway
//!
//! OpenAPI REST gateway exposing SPINE agentic web APIs to non-Rust clients.
//! Provides session management, navigation, search, HLS execution, and health
//! endpoints with auto-generated Swagger UI.
//!
//! Also bridges SPINE's native agentic frame types
//! ([`spine_protocol::StreamStart`] / [`StreamToken`] / [`StreamEnd`] and
//! the [`CapabilityAdvertisement`] handshake) to the HTTP/SSE wire format
//! every OpenAI-compatible client speaks. See the [`agentic_sse`] module.

mod agentic_sse;
mod auth;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::response::{Json, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

use spine_agent::AgentClient;
use spine_compiler::Compiler;
use spine_core::SpineConfig;
use spine_parser::parse_html;
use tracing::instrument;

// ---------------------------------------------------------------------------
// OpenAPI schema
// ---------------------------------------------------------------------------

#[derive(OpenApi)]
#[openapi(
    paths(
        create_session,
        delete_session,
        navigate,
        get_ur,
        get_html,
        search,
        execute_hls,
        ping_session,
        parse_html_endpoint,
        compile_hls,
        health,
        ready,
    ),
    components(schemas(
        CreateSessionReq, SessionInfo, NavigateReq, SearchReq,
        ExecuteHlsReq, ParseHtmlReq, CompileReq,
        UrResponse, HtmlResponse, SearchResponse,
        ExecResponse, PingResponse, ParseResponse,
        CompileResponse, HealthResponse, ReadyResponse,
        ErrorResponse,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "sessions", description = "Session lifecycle"),
        (name = "browse", description = "Navigation & content"),
        (name = "compute", description = "HLS / WASM execution"),
        (name = "ops", description = "Health & readiness"),
    )
)]
struct ApiDoc;

struct SecurityAddon;
impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(schema) = openapi.components.as_mut() {
            schema.add_security_scheme(
                "bearer",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("UUID")
                        .build(),
                ),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

type Session = Arc<Mutex<AgentClient<TcpStream>>>;

struct AppState {
    sessions: DashMap<Uuid, Session>,
    backend_addr: String,
    start_time: Instant,
    max_sessions: usize,
    #[allow(dead_code)]
    tls_config: spine_core::config::TlsConfig,
    requests_total: AtomicU64,
    errors_total: AtomicU64,
}

impl AppState {
    fn new(config: &SpineConfig) -> Self {
        Self {
            sessions: DashMap::new(),
            backend_addr: format!("{}:{}", config.server.host, config.server.port),
            start_time: Instant::now(),
            max_sessions: config.server.max_sessions,
            tls_config: config.tls.clone(),
            requests_total: AtomicU64::new(0),
            errors_total: AtomicU64::new(0),
        }
    }
}

// ---------------------------------------------------------------------------
// Request / response DTOs
// ---------------------------------------------------------------------------

#[derive(Deserialize, ToSchema)]
struct CreateSessionReq {
    /// Optional backend address override (default: config value)
    #[serde(default)]
    backend_addr: Option<String>,
}

#[derive(Serialize, ToSchema)]
struct SessionInfo {
    session_id: Uuid,
    backend: String,
}

#[derive(Deserialize, ToSchema)]
struct NavigateReq {
    url: String,
}

#[derive(Deserialize, ToSchema)]
struct SearchReq {
    query: String,
}

#[derive(Deserialize, ToSchema)]
struct ExecuteHlsReq {
    script: String,
}

#[derive(Deserialize, ToSchema)]
struct ParseHtmlReq {
    html: String,
}

#[derive(Deserialize, ToSchema)]
struct CompileReq {
    source: String,
}

#[derive(Serialize, ToSchema)]
struct UrResponse {
    title: String,
    element_count: usize,
    metadata: std::collections::HashMap<String, String>,
    raw: serde_json::Value,
}

#[derive(Serialize, ToSchema)]
struct HtmlResponse {
    html: String,
}

#[derive(Serialize, ToSchema)]
struct SearchResponse {
    results: serde_json::Value,
}

#[derive(Serialize, ToSchema)]
struct ExecResponse {
    result: serde_json::Value,
}

#[derive(Serialize, ToSchema)]
struct PingResponse {
    round_trip_ms: u64,
}

#[derive(Serialize, ToSchema)]
struct ParseResponse {
    title: String,
    element_count: usize,
    metadata: std::collections::HashMap<String, String>,
}

#[derive(Serialize, ToSchema)]
struct CompileResponse {
    instruction_count: usize,
    data_bytes: usize,
    exported_functions: Vec<String>,
    capabilities: Vec<String>,
}

#[derive(Serialize, ToSchema)]
struct HealthResponse {
    status: String,
    uptime_secs: u64,
    active_sessions: usize,
}

#[derive(Serialize, ToSchema)]
struct ReadyResponse {
    ready: bool,
    available_slots: usize,
}

#[derive(Serialize, ToSchema)]
struct ErrorResponse {
    error: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn err(status: StatusCode, msg: impl ToString) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            error: msg.to_string(),
        }),
    )
}

fn get_session(state: &AppState, id: Uuid) -> Result<Session, (StatusCode, Json<ErrorResponse>)> {
    state
        .sessions
        .get(&id)
        .map(|s| Arc::clone(s.value()))
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "session not found"))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Create a new agent session
#[utoipa::path(
    post, path = "/api/sessions",
    tag = "sessions",
    request_body = CreateSessionReq,
    responses(
        (status = 201, description = "Session created", body = SessionInfo),
        (status = 503, description = "Capacity exceeded", body = ErrorResponse),
    )
)]
async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionReq>,
) -> Result<(StatusCode, Json<SessionInfo>), (StatusCode, Json<ErrorResponse>)> {
    state.requests_total.fetch_add(1, Ordering::Relaxed);
    if state.sessions.len() >= state.max_sessions {
        state.errors_total.fetch_add(1, Ordering::Relaxed);
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "max sessions reached"));
    }
    let addr = req.backend_addr.as_deref().unwrap_or(&state.backend_addr);
    let client = AgentClient::connect(addr)
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("connect failed: {e}")))?;
    let id = Uuid::new_v4();
    state.sessions.insert(id, Arc::new(Mutex::new(client)));
    Ok((
        StatusCode::CREATED,
        Json(SessionInfo {
            session_id: id,
            backend: addr.to_string(),
        }),
    ))
}

/// Delete a session
#[utoipa::path(
    delete, path = "/api/sessions/{id}",
    tag = "sessions",
    params(("id" = Uuid, Path, description = "Session UUID")),
    responses(
        (status = 204, description = "Session deleted"),
        (status = 404, description = "Not found", body = ErrorResponse),
    )
)]
async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    state
        .sessions
        .remove(&id)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "session not found"))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Navigate to a URL
#[utoipa::path(
    post, path = "/api/sessions/{id}/navigate",
    tag = "browse",
    params(("id" = Uuid, Path, description = "Session UUID")),
    request_body = NavigateReq,
    responses(
        (status = 200, description = "Navigated"),
        (status = 404, description = "Session not found", body = ErrorResponse),
        (status = 502, description = "Backend error", body = ErrorResponse),
    )
)]
#[instrument(skip_all, fields(id = %id))]
async fn navigate(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<NavigateReq>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    state.requests_total.fetch_add(1, Ordering::Relaxed);
    let session = get_session(&state, id)?;
    let mut client = session.lock().await;
    client
        .navigate(&req.url)
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(StatusCode::OK)
}

/// Get Unified Representation of current page
#[utoipa::path(
    get, path = "/api/sessions/{id}/ur",
    tag = "browse",
    params(("id" = Uuid, Path, description = "Session UUID")),
    responses(
        (status = 200, body = UrResponse),
        (status = 404, body = ErrorResponse),
        (status = 502, body = ErrorResponse),
    )
)]
async fn get_ur(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<UrResponse>, (StatusCode, Json<ErrorResponse>)> {
    let session = get_session(&state, id)?;
    let mut client = session.lock().await;
    let ur = client
        .get_ur()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(UrResponse {
        title: ur.title.clone(),
        element_count: ur.elements.len(),
        metadata: ur.metadata.clone(),
        raw: serde_json::to_value(&ur).unwrap_or_default(),
    }))
}

/// Get raw HTML of current page
#[utoipa::path(
    get, path = "/api/sessions/{id}/html",
    tag = "browse",
    params(("id" = Uuid, Path, description = "Session UUID")),
    responses(
        (status = 200, body = HtmlResponse),
        (status = 404, body = ErrorResponse),
        (status = 502, body = ErrorResponse),
    )
)]
async fn get_html(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<HtmlResponse>, (StatusCode, Json<ErrorResponse>)> {
    let session = get_session(&state, id)?;
    let mut client = session.lock().await;
    let html = client
        .get_raw_html()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(HtmlResponse { html }))
}

/// Search across the web
#[utoipa::path(
    post, path = "/api/sessions/{id}/search",
    tag = "browse",
    params(("id" = Uuid, Path, description = "Session UUID")),
    request_body = SearchReq,
    responses(
        (status = 200, body = SearchResponse),
        (status = 404, body = ErrorResponse),
        (status = 502, body = ErrorResponse),
    )
)]
#[instrument(skip_all, fields(id = %id))]
async fn search(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<SearchReq>,
) -> Result<Json<SearchResponse>, (StatusCode, Json<ErrorResponse>)> {
    state.requests_total.fetch_add(1, Ordering::Relaxed);
    let session = get_session(&state, id)?;
    let mut client = session.lock().await;
    let results = client
        .search(&req.query)
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(SearchResponse { results }))
}

/// Execute an HLS script
#[utoipa::path(
    post, path = "/api/sessions/{id}/execute",
    tag = "compute",
    params(("id" = Uuid, Path, description = "Session UUID")),
    request_body = ExecuteHlsReq,
    responses(
        (status = 200, body = ExecResponse),
        (status = 404, body = ErrorResponse),
        (status = 502, body = ErrorResponse),
    )
)]
#[instrument(skip_all, fields(id = %id))]
async fn execute_hls(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<ExecuteHlsReq>,
) -> Result<Json<ExecResponse>, (StatusCode, Json<ErrorResponse>)> {
    state.requests_total.fetch_add(1, Ordering::Relaxed);
    let session = get_session(&state, id)?;
    let mut client = session.lock().await;
    let result = client
        .execute_hls(&req.script)
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(ExecResponse {
        result: serde_json::to_value(&result).unwrap_or_default(),
    }))
}

/// Ping a session (measure round-trip)
#[utoipa::path(
    get, path = "/api/sessions/{id}/ping",
    tag = "browse",
    params(("id" = Uuid, Path, description = "Session UUID")),
    responses(
        (status = 200, body = PingResponse),
        (status = 404, body = ErrorResponse),
        (status = 502, body = ErrorResponse),
    )
)]
async fn ping_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<PingResponse>, (StatusCode, Json<ErrorResponse>)> {
    let session = get_session(&state, id)?;
    let mut client = session.lock().await;
    let ms = client
        .ping()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(PingResponse { round_trip_ms: ms }))
}

/// Parse HTML to Unified Representation (stateless)
#[utoipa::path(
    post, path = "/api/parse",
    tag = "compute",
    request_body = ParseHtmlReq,
    responses(
        (status = 200, body = ParseResponse),
        (status = 400, body = ErrorResponse),
    )
)]
async fn parse_html_endpoint(
    Json(req): Json<ParseHtmlReq>,
) -> Result<Json<ParseResponse>, (StatusCode, Json<ErrorResponse>)> {
    let ur = parse_html(&req.html).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(ParseResponse {
        title: ur.title,
        element_count: ur.elements.len(),
        metadata: ur.metadata,
    }))
}

/// Compile HLS source to binary (stateless)
#[utoipa::path(
    post, path = "/api/compile",
    tag = "compute",
    request_body = CompileReq,
    responses(
        (status = 200, body = CompileResponse),
        (status = 400, body = ErrorResponse),
    )
)]
async fn compile_hls(
    Json(req): Json<CompileReq>,
) -> Result<Json<CompileResponse>, (StatusCode, Json<ErrorResponse>)> {
    let binary = Compiler::compile(&req.source).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(CompileResponse {
        instruction_count: binary.instructions.len(),
        data_bytes: binary.data.len(),
        exported_functions: binary.exported_functions.keys().cloned().collect(),
        capabilities: binary.capabilities,
    }))
}

/// Health check
#[utoipa::path(
    get, path = "/health",
    tag = "ops",
    responses((status = 200, body = HealthResponse))
)]
async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        uptime_secs: state.start_time.elapsed().as_secs(),
        active_sessions: state.sessions.len(),
    })
}

/// Readiness check
#[utoipa::path(
    get, path = "/ready",
    tag = "ops",
    responses((status = 200, body = ReadyResponse))
)]
async fn ready(State(state): State<Arc<AppState>>) -> Json<ReadyResponse> {
    let available = state.max_sessions.saturating_sub(state.sessions.len());
    Json(ReadyResponse {
        ready: available > 0,
        available_slots: available,
    })
}

/// Prometheus-compatible metrics endpoint
async fn metrics(State(state): State<Arc<AppState>>) -> Response {
    let uptime = state.start_time.elapsed().as_secs();
    let sessions = state.sessions.len();
    let requests = state.requests_total.load(Ordering::Relaxed);
    let errors = state.errors_total.load(Ordering::Relaxed);

    let body = format!(
        "# HELP spine_gateway_uptime_seconds Gateway uptime in seconds\n\
         # TYPE spine_gateway_uptime_seconds gauge\n\
         spine_gateway_uptime_seconds {}\n\
         # HELP spine_gateway_active_sessions Number of active sessions\n\
         # TYPE spine_gateway_active_sessions gauge\n\
         spine_gateway_active_sessions {}\n\
         # HELP spine_gateway_requests_total Total API requests\n\
         # TYPE spine_gateway_requests_total counter\n\
         spine_gateway_requests_total {}\n\
         # HELP spine_gateway_errors_total Total API errors\n\
         # TYPE spine_gateway_errors_total counter\n\
         spine_gateway_errors_total {}\n",
        uptime, sessions, requests, errors
    );

    Response::builder()
        .header("content-type", "text/plain; version=0.0.4")
        .body(body.into())
        .unwrap()
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

/// Install the FIPS-shaped rustls `CryptoProvider` when the `fips`
/// feature is enabled at compile time. With `fips` off this is a
/// no-op and rustls keeps using the default `ring` backend.
///
/// The actual FIPS-validated module is provided by AWS-LC compiled
/// with `AWS_LC_FIPS=1`; this function only wires the integration
/// point. See `SECURITY_AUDIT.md § 2` for the deployer toolchain.
#[cfg(feature = "fips")]
fn install_fips_provider() -> anyhow::Result<()> {
    use anyhow::anyhow;
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| {
            anyhow!(
                "could not install aws-lc-rs as the rustls default CryptoProvider \
                 — another provider is already installed in this process"
            )
        })?;
    tracing::info!(
        "FIPS mode: aws-lc-rs CryptoProvider installed as rustls default. \
         For an end-to-end FIPS-validated build, also rebuild aws-lc-rs with \
         AWS_LC_FIPS=1."
    );
    Ok(())
}

#[cfg(not(feature = "fips"))]
fn install_fips_provider() -> anyhow::Result<()> {
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // Install the FIPS provider before anything else touches rustls —
    // CryptoProvider can only be installed once per process and must
    // happen before the first TLS handshake.
    install_fips_provider()?;

    let config = SpineConfig::load();
    let gateway_port = config.server.metrics_port + 1; // 9091

    let state = Arc::new(AppState::new(&config));

    let app = Router::new()
        // Session management
        .route("/api/sessions", post(create_session))
        .route("/api/sessions/{id}", delete(delete_session))
        // Browse
        .route("/api/sessions/{id}/navigate", post(navigate))
        .route("/api/sessions/{id}/ur", get(get_ur))
        .route("/api/sessions/{id}/html", get(get_html))
        .route("/api/sessions/{id}/search", post(search))
        .route("/api/sessions/{id}/ping", get(ping_session))
        // Compute
        .route("/api/sessions/{id}/execute", post(execute_hls))
        .route("/api/parse", post(parse_html_endpoint))
        .route("/api/compile", post(compile_hls))
        // Agentic surface — OpenAI-compatible chat/completions + capability
        // discovery. Bridges SPINE StreamToken frames to SSE.
        .route(
            "/v1/chat/completions",
            post(agentic_sse::chat_completions_stream),
        )
        .route("/v1/agentic/capabilities", get(agentic_sse::capabilities))
        // Neural encoder-decoder protocols — embeddings + codec catalog.
        .route("/v1/embeddings", post(agentic_sse::embeddings))
        .route("/v1/agentic/codecs", get(agentic_sse::codecs))
        // Ops
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/metrics", get(metrics))
        // Swagger UI
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        // Bound every POST body so an unbounded request cannot exhaust
        // gateway RAM before any handler validates it. 8 MiB is enough
        // for embedding batches of a few hundred sentences or for
        // moderately-sized HLS programs; deployments that need larger
        // bodies should override per-route.
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(state);

    // v1.3.0 secure-by-default auth contract: the gateway refuses to
    // start unless the deployer has made an explicit choice via
    // SPINE_GATEWAY_BEARER_TOKEN (recommended) or
    // SPINE_GATEWAY_ALLOW_UNAUTH=1 (explicit dev-mode opt-out). See
    // src/auth.rs for the full contract.
    let mode = match auth::AuthMode::resolve() {
        Ok(m) => m,
        Err(e) => {
            // Don't go through tracing — the deployer needs to see this
            // even if their subscriber isn't wired up yet.
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let app = match mode {
        auth::AuthMode::Bearer(cfg) => {
            tracing::info!("Bearer-token auth ENABLED (SPINE_GATEWAY_BEARER_TOKEN set)");
            let cfg = cfg.clone();
            app.layer(axum::middleware::from_fn(move |req, next| {
                let cfg = cfg.clone();
                async move { auth::require_bearer(cfg, req, next).await }
            }))
        }
        auth::AuthMode::Unauthenticated => {
            tracing::warn!(
                "Bearer-token auth DISABLED via SPINE_GATEWAY_ALLOW_UNAUTH=1. \
                 Public exposure without auth is appropriate ONLY for local \
                 dev or when an upstream proxy authenticates."
            );
            app
        }
    };

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{gateway_port}")).await?;
    tracing::info!("SPINE Gateway listening on http://0.0.0.0:{gateway_port}");
    tracing::info!("Swagger UI: http://0.0.0.0:{gateway_port}/swagger-ui/");
    tracing::info!("Backend: {}:{}", config.server.host, config.server.port);

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("Gateway shutting down...");
        })
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openapi_schema_valid() {
        let doc = ApiDoc::openapi();
        let json = serde_json::to_string_pretty(&doc).unwrap();
        assert!(json.contains("/api/sessions"));
        assert!(json.contains("/api/sessions"));
        assert!(json.contains("/health"));
    }

    #[test]
    fn test_error_response() {
        let (status, Json(body)) = err(StatusCode::NOT_FOUND, "gone");
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body.error, "gone");
    }

    #[test]
    fn test_health_response_serialization() {
        let h = HealthResponse {
            status: "ok".into(),
            uptime_secs: 42,
            active_sessions: 3,
        };
        let json = serde_json::to_string(&h).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
    }

    #[test]
    fn test_compile_hls_offline() {
        let binary = Compiler::compile("let x = 1 + 2; x").unwrap();
        assert!(!binary.instructions.is_empty());
    }

    #[test]
    fn test_parse_html_offline() {
        let ur =
            parse_html("<html><head><title>Test</title></head><body><p>Hello</p></body></html>")
                .unwrap();
        assert_eq!(ur.title, "Test");
    }

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
            || std::path::Path::new("../deploy/grafana/spine-dashboard.json").exists()
            || std::path::Path::new("../../deploy/grafana/spine-dashboard.json").exists());
    }

    #[test]
    fn test_session_info_serialization() {
        let id = Uuid::new_v4();
        let info = SessionInfo {
            session_id: id,
            backend: "127.0.0.1:9000".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains(&id.to_string()));
        assert!(json.contains("127.0.0.1:9000"));
    }

    #[test]
    fn test_navigate_req_deserialization() {
        let json = r#"{"url":"https://example.com"}"#;
        let req: NavigateReq = serde_json::from_str(json).unwrap();
        assert_eq!(req.url, "https://example.com");
    }

    #[test]
    fn test_search_req_deserialization() {
        let json = r#"{"query":"rust programming"}"#;
        let req: SearchReq = serde_json::from_str(json).unwrap();
        assert_eq!(req.query, "rust programming");
    }

    #[test]
    fn test_execute_hls_req_deserialization() {
        let json = r#"{"script":"let x = 1 + 2"}"#;
        let req: ExecuteHlsReq = serde_json::from_str(json).unwrap();
        assert_eq!(req.script, "let x = 1 + 2");
    }

    #[test]
    fn test_parse_html_req_deserialization() {
        let json = r#"{"html":"<p>Hello</p>"}"#;
        let req: ParseHtmlReq = serde_json::from_str(json).unwrap();
        assert_eq!(req.html, "<p>Hello</p>");
    }

    #[test]
    fn test_compile_req_deserialization() {
        let json = r#"{"source":"let x = 42"}"#;
        let req: CompileReq = serde_json::from_str(json).unwrap();
        assert_eq!(req.source, "let x = 42");
    }

    #[test]
    fn test_ur_response_serialization() {
        let resp = UrResponse {
            title: "Test".into(),
            element_count: 5,
            metadata: std::collections::HashMap::new(),
            raw: serde_json::json!({"elements": []}),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"title\":\"Test\""));
        assert!(json.contains("\"element_count\":5"));
    }

    #[test]
    fn test_ready_response_serialization() {
        let resp = ReadyResponse {
            ready: true,
            available_slots: 42,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"ready\":true"));
        assert!(json.contains("\"available_slots\":42"));
    }

    #[test]
    fn test_compile_response_serialization() {
        let resp = CompileResponse {
            instruction_count: 10,
            data_bytes: 256,
            exported_functions: vec!["render".into()],
            capabilities: vec!["network".into()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"instruction_count\":10"));
        assert!(json.contains("\"render\""));
    }

    #[test]
    fn test_ping_response_serialization() {
        let resp = PingResponse { round_trip_ms: 42 };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"round_trip_ms\":42"));
    }

    #[test]
    fn test_get_session_not_found() {
        let config = SpineConfig::default();
        let state = AppState::new(&config);
        let result = get_session(&state, Uuid::new_v4());
        assert!(result.is_err());
    }

    #[test]
    fn test_create_session_req_default_addr() {
        let json = r#"{}"#;
        let req: CreateSessionReq = serde_json::from_str(json).unwrap();
        assert!(req.backend_addr.is_none());
    }

    #[test]
    fn test_create_session_req_custom_addr() {
        let json = r#"{"backend_addr":"10.0.0.1:9000"}"#;
        let req: CreateSessionReq = serde_json::from_str(json).unwrap();
        assert_eq!(req.backend_addr.unwrap(), "10.0.0.1:9000");
    }

    #[test]
    fn test_openapi_paths_coverage() {
        let doc = ApiDoc::openapi();
        let json = serde_json::to_string(&doc).unwrap();
        // Verify all documented routes exist
        assert!(json.contains("/api/sessions"));
        assert!(json.contains("/health"));
        assert!(json.contains("/ready"));
        assert!(json.contains("/api/parse"));
        assert!(json.contains("/api/compile"));
    }

    #[test]
    fn test_max_sessions_from_config() {
        let mut config = SpineConfig::default();
        config.server.max_sessions = 100;
        let state = AppState::new(&config);
        assert_eq!(state.max_sessions, 100);
    }

}
