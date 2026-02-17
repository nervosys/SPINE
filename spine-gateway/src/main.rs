//! # SPINE Gateway
//!
//! OpenAPI REST gateway exposing SPINE agentic web APIs to non-Rust clients.
//! Provides session management, navigation, search, HLS execution, and health
//! endpoints with auto-generated Swagger UI.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
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
}

impl AppState {
    fn new(config: &SpineConfig) -> Self {
        Self {
            sessions: DashMap::new(),
            backend_addr: format!("{}:{}", config.server.host, config.server.port),
            start_time: Instant::now(),
            max_sessions: config.server.max_sessions,
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
    if state.sessions.len() >= state.max_sessions {
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
async fn navigate(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<NavigateReq>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
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
async fn search(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<SearchReq>,
) -> Result<Json<SearchResponse>, (StatusCode, Json<ErrorResponse>)> {
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
async fn execute_hls(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<ExecuteHlsReq>,
) -> Result<Json<ExecResponse>, (StatusCode, Json<ErrorResponse>)> {
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

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

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
        // Ops
        .route("/health", get(health))
        .route("/ready", get(ready))
        // Swagger UI
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{gateway_port}")).await?;
    log::info!("SPINE Gateway listening on http://0.0.0.0:{gateway_port}");
    log::info!("Swagger UI: http://0.0.0.0:{gateway_port}/swagger-ui/");
    log::info!("Backend: {}:{}", config.server.host, config.server.port);

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            log::info!("Gateway shutting down...");
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
}
