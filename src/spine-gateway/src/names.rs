//! The `spine://` namespace over plain HTTP.
//!
//! Resolution already works over SPINE's own protocol. This module exists
//! because most of the world cannot speak it: a client with nothing but an HTTP
//! stack should still be able to resolve a name, find who provides a capability,
//! and publish a record. That is the cheapest interop the namespace has, and it
//! costs nothing in trust — records are signed, so a client that verifies one is
//! not trusting this gateway, only using it as a lens.
//!
//! ## Mapping the namespace onto HTTP honestly
//!
//! The temptation is to return `200 {"error": ...}` for everything and let the
//! body carry the meaning. Instead each namespace outcome maps to the status
//! code that already means it: a resolved name is `200`, an unchanged one is
//! `304`, a name nobody has published is `404`, and a malformed one is `400`.
//! An HTTP client's existing retry and cache logic then does the right thing
//! without knowing anything about SPINE.
//!
//! ## What the gateway does not launder
//!
//! Every resolution carries its provenance through to the response, in the body
//! and in `X-Spine-Provenance`. It matters most for `host:` names, whose
//! `Address` provenance means *nothing attested this* — the address was simply
//! read out of the name. Folding that into an ordinary `200` would quietly hand
//! an HTTP client the one binding in the namespace that is nobody's word, in the
//! same shape as bindings that carry a signature.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use utoipa::{IntoParams, ToSchema};

use spine_agent::AgentClient;
use spine_protocol::{NameProvenance, NameResolution};

use crate::{err, AppState, ErrorResponse};

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Deserialize, IntoParams)]
pub struct ResolveParams {
    /// The `spine://` name to resolve.
    pub name: String,
    /// A content hash the caller already holds, hex-encoded. When it still
    /// matches, the answer is `304` with no body.
    #[serde(default)]
    pub if_none_match: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct ResolveNamesReq {
    /// Names to resolve in one round trip. Each resolves independently, so one
    /// bad name does not fail the rest.
    pub names: Vec<String>,
}

#[derive(Deserialize, IntoParams)]
pub struct ProvidersParams {
    /// The capability term, e.g. `web.search`.
    pub capability: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Deserialize, ToSchema)]
pub struct PublishReq {
    /// A JSON-encoded signed `NameRecord`.
    pub record: serde_json::Value,
}

#[derive(Deserialize, IntoParams)]
pub struct CrawlParams {
    /// The name to start from.
    pub seed: String,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub max_visits: Option<usize>,
}

#[derive(Serialize, ToSchema)]
pub struct ResolvedResponse {
    /// The signed record, verifiable against the name without trusting this
    /// gateway.
    pub record: serde_json::Value,
    /// Where the answer came from: `Cache`, `StaleCache`, `Local`, `Network`,
    /// or `Address`.
    pub provenance: String,
    /// Whether anything actually vouched for this binding. False for `host:`
    /// names, whose address is simply read out of the name itself.
    pub attested: bool,
}

#[derive(Serialize, ToSchema)]
pub struct ResolveBatchResponse {
    /// One entry per requested name, in the order asked.
    pub resolutions: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Idle origin connections, kept so a namespace request need not open one.
///
/// Resolution is stateless, so these endpoints deliberately take no session:
/// requiring a client to create one, resolve, and tear it down would be three
/// round trips to answer a question that needs one. That left a TCP connect in
/// front of every request, which this removes.
///
/// A plain free-list rather than a fair pool. There is no fixed connection count
/// to queue behind — under load the gateway simply opens more and keeps only up
/// to `MAX_IDLE` of them afterwards — so nothing here can starve and nothing
/// needs to wait its turn.
pub struct OriginPool<C = AgentClient<TcpStream>> {
    idle: std::sync::Mutex<Vec<C>>,
}

// Written out rather than derived: `derive(Default)` would demand `C: Default`,
// and an origin connection has no default. The empty pool does.
impl<C> Default for OriginPool<C> {
    fn default() -> Self {
        Self {
            idle: std::sync::Mutex::new(Vec::new()),
        }
    }
}

/// Idle connections retained. Past this a finished connection is dropped rather
/// than hoarded: the origin caps its own sessions, so a gateway holding idle
/// connections open is spending the origin's budget to save itself a connect it
/// may never make.
const MAX_IDLE: usize = 8;

impl<C> OriginPool<C> {
    fn take(&self) -> Option<C> {
        self.idle.lock().ok()?.pop()
    }

    fn put(&self, client: C) {
        if let Ok(mut idle) = self.idle.lock() {
            if idle.len() < MAX_IDLE {
                idle.push(client);
            }
        }
    }

    /// Idle connections currently held.
    pub fn idle_count(&self) -> usize {
        self.idle.lock().map(|i| i.len()).unwrap_or(0)
    }
}

type OriginFut<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<T>> + Send + 'a>>;

/// Run one namespace command against the origin, reusing a pooled connection.
///
/// A pooled connection may have been closed since it was last used — the origin
/// reaps idle sessions on its own schedule — and that is not the caller's fault.
/// So a failure on a *pooled* connection is retried once on a fresh one, and only
/// a failure on a connection this call opened is reported. That is the same
/// single transparent re-dial the mesh transport already does, for the same
/// reason.
///
/// A connection is returned to the pool only after a successful call. One that
/// failed is dropped: it may be half-closed or stopped mid-frame, and handing it
/// to the next request would spread one failure across two.
async fn with_origin<T, F>(state: &AppState, mut op: F) -> anyhow::Result<T>
where
    F: for<'a> FnMut(&'a mut AgentClient<TcpStream>) -> OriginFut<'a, T>,
{
    if let Some(mut client) = state.origin_pool.take() {
        if let Ok(value) = op(&mut client).await {
            state.origin_pool.put(client);
            return Ok(value);
        }
        // Fall through and try once on a connection we know is fresh.
    }

    let mut client = AgentClient::connect(&state.backend_addr)
        .await
        .map_err(|e| anyhow::anyhow!("origin unreachable: {e}"))?;
    let value = op(&mut client).await?;
    state.origin_pool.put(client);
    Ok(value)
}

/// Map an origin failure to a gateway response.
fn upstream(e: anyhow::Error) -> (StatusCode, Json<ErrorResponse>) {
    err(StatusCode::BAD_GATEWAY, e)
}

/// Decode a hex content hash from `if_none_match`.
///
/// A malformed hash is refused rather than ignored. Ignoring it would answer
/// with a full body the caller may already hold, which is merely wasteful — but
/// it would also silently turn a conditional request into an unconditional one,
/// and the caller would never learn its cache validator was garbage.
fn parse_hash(raw: &Option<String>) -> Result<Option<[u8; 32]>, (StatusCode, Json<ErrorResponse>)> {
    let Some(text) = raw else { return Ok(None) };
    let text = text.trim().trim_matches('"');
    if text.len() != 64 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "if_none_match must be 64 hex characters",
        ));
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)
            .map_err(|_| err(StatusCode::BAD_REQUEST, "if_none_match is not hex"))?;
    }
    Ok(Some(out))
}

fn provenance_name(p: NameProvenance) -> &'static str {
    match p {
        NameProvenance::Cache => "Cache",
        NameProvenance::StaleCache => "StaleCache",
        NameProvenance::Local => "Local",
        NameProvenance::Network => "Network",
        NameProvenance::Address => "Address",
    }
}

/// Whether anything vouched for the binding.
///
/// Only `Address` is unattested, and it is unattested completely: the address
/// came out of the name, nobody signed it, and nobody was asked. Every other
/// provenance describes *where* a signed record was found, which is a question
/// about latency, not about trust.
fn is_attested(p: NameProvenance) -> bool {
    !matches!(p, NameProvenance::Address)
}

/// Turn a namespace outcome into the HTTP status that already means it.
fn into_response(resolution: NameResolution) -> Response {
    match resolution {
        NameResolution::Resolved { record, provenance } => {
            let mut headers = HeaderMap::new();
            if let Ok(value) = HeaderValue::from_str(provenance_name(provenance)) {
                headers.insert("x-spine-provenance", value);
            }
            (
                StatusCode::OK,
                headers,
                Json(ResolvedResponse {
                    record,
                    provenance: provenance_name(provenance).to_string(),
                    attested: is_attested(provenance),
                }),
            )
                .into_response()
        }
        // 304 carries no body by definition, so the freshness the responder
        // granted has to travel in a header or be lost.
        NameResolution::Unchanged { ttl_secs } => {
            let mut headers = HeaderMap::new();
            if let Ok(value) = HeaderValue::from_str(&format!("max-age={ttl_secs}")) {
                headers.insert(header::CACHE_CONTROL, value);
            }
            (StatusCode::NOT_MODIFIED, headers).into_response()
        }
        NameResolution::NotFound { name } => err(
            StatusCode::NOT_FOUND,
            format!("no record published for {name}"),
        )
        .into_response(),
        // The name itself is wrong, which is the caller's error and not a
        // missing resource — a 404 here would send them looking for a publisher
        // that could never exist.
        NameResolution::Invalid { name, reason } => {
            err(StatusCode::BAD_REQUEST, format!("{name}: {reason}")).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Resolve a `spine://` name to its signed record
#[utoipa::path(
    get, path = "/v1/names/resolve",
    tag = "names",
    params(ResolveParams),
    responses(
        (status = 200, body = ResolvedResponse, description = "Resolved; the record verifies against the name"),
        (status = 304, description = "The caller's content hash still matches"),
        (status = 400, body = ErrorResponse, description = "Malformed name or validator"),
        (status = 404, body = ErrorResponse, description = "Nobody has published that name"),
        (status = 502, body = ErrorResponse),
    )
)]
pub async fn resolve_name(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ResolveParams>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let if_none_match = parse_hash(&params.if_none_match)?;
    let name = params.name.clone();
    let resolution = with_origin(&state, move |c| {
        let name = name.clone();
        Box::pin(async move { c.resolve_name(&name, if_none_match).await })
    })
    .await
    .map_err(upstream)?;
    Ok(into_response(resolution))
}

/// Resolve many names in one round trip
#[utoipa::path(
    post, path = "/v1/names/resolve",
    tag = "names",
    request_body = ResolveNamesReq,
    responses(
        (status = 200, body = ResolveBatchResponse),
        (status = 502, body = ErrorResponse),
    )
)]
pub async fn resolve_names(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ResolveNamesReq>,
) -> Result<Json<ResolveBatchResponse>, (StatusCode, Json<ErrorResponse>)> {
    let names = req.names.clone();
    let resolutions = with_origin(&state, move |c| {
        let names = names.clone();
        Box::pin(async move { c.resolve_names(&names).await })
    })
    .await
    .map_err(upstream)?;
    // The batch itself always succeeds; per-name outcomes travel in the body,
    // because one unresolvable name in a dozen is not a failed request.
    Ok(Json(ResolveBatchResponse {
        resolutions: serde_json::to_value(resolutions).unwrap_or_default(),
    }))
}

/// Find providers of a capability
#[utoipa::path(
    get, path = "/v1/names/providers",
    tag = "names",
    params(ProvidersParams),
    responses(
        (status = 200, description = "Ranked providers, each a signed record"),
        (status = 502, body = ErrorResponse),
    )
)]
pub async fn find_providers(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ProvidersParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let capability = params.capability.clone();
    let limit = params.limit;
    let body = with_origin(&state, move |c| {
        let capability = capability.clone();
        Box::pin(async move { c.find_providers(&capability, limit).await })
    })
    .await
    .map_err(upstream)?;
    Ok(Json(body))
}

/// Publish a signed record
#[utoipa::path(
    post, path = "/v1/names/publish",
    tag = "names",
    request_body = PublishReq,
    responses(
        (status = 201, description = "Published; the body reports how many replicas landed"),
        (status = 400, body = ErrorResponse, description = "Unsigned, forged, or malformed record"),
        (status = 502, body = ErrorResponse),
    )
)]
pub async fn publish_name(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PublishReq>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    // A rejected record is the caller's problem, not the origin's: the origin
    // verified the signature and found it wanting, which is a 400 and not the
    // 502 that a naive error passthrough would produce.
    let record = req.record.clone();
    let body = with_origin(&state, move |c| {
        let record = record.clone();
        Box::pin(async move { c.publish_name(record).await })
    })
    .await
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok((StatusCode::CREATED, Json(body)))
}

/// Resolve a name and get the endpoints to dial, in priority order
#[utoipa::path(
    get, path = "/v1/names/endpoints",
    tag = "names",
    params(ResolveParams),
    responses(
        (status = 200, description = "The resolution and its endpoints, highest priority first"),
        (status = 400, body = ErrorResponse),
        (status = 502, body = ErrorResponse),
    )
)]
pub async fn fetch_name(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ResolveParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let if_none_match = parse_hash(&params.if_none_match)?;
    // Stops at the endpoints rather than dialing them. Fetching the bytes is
    // the caller's business, and folding it in here would hide which half of
    // "resolve, then fetch" failed.
    let name = params.name.clone();
    let body = with_origin(&state, move |c| {
        let name = name.clone();
        Box::pin(async move { c.fetch_name(&name, if_none_match).await })
    })
    .await
    .map_err(upstream)?;
    Ok(Json(body))
}

/// Walk the agent web from a seed name
#[utoipa::path(
    get, path = "/v1/names/crawl",
    tag = "names",
    params(CrawlParams),
    responses(
        (status = 200, description = "Visited names, plus what the budget skipped"),
        (status = 502, body = ErrorResponse),
    )
)]
pub async fn crawl_names(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CrawlParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let seed = params.seed.clone();
    let (max_depth, max_visits) = (params.max_depth, params.max_visits);
    let body = with_origin(&state, move |c| {
        let seed = seed.clone();
        Box::pin(async move { c.crawl_names(&seed, max_depth, max_visits).await })
    })
    .await
    .map_err(upstream)?;
    Ok(Json(body))
}

#[cfg(test)]
mod tests {
    use super::*;


    /// The cap is the whole reason `put` is not just `push`. The origin limits
    /// its own sessions, so a gateway that hoarded idle connections would be
    /// spending the origin's budget on connects it may never make.
    #[test]
    fn the_pool_stops_accepting_connections_at_its_cap() {
        let pool: OriginPool<u32> = OriginPool::default();
        for i in 0..(MAX_IDLE as u32 * 4) {
            pool.put(i);
        }
        assert_eq!(pool.idle_count(), MAX_IDLE);
    }

    /// A taken connection must leave the pool, or two requests would end up
    /// writing down the same socket.
    #[test]
    fn taking_a_connection_removes_it_from_the_pool() {
        let pool: OriginPool<u32> = OriginPool::default();
        pool.put(7);
        assert_eq!(pool.idle_count(), 1);
        assert_eq!(pool.take(), Some(7));
        assert_eq!(pool.idle_count(), 0);
        assert_eq!(pool.take(), None, "an empty pool hands out nothing");
    }

    /// Most-recently-returned first. The connection idle for the shortest time
    /// is the one likeliest to still be open, and the origin reaps by idleness.
    #[test]
    fn the_pool_hands_back_the_most_recently_returned_connection() {
        let pool: OriginPool<u32> = OriginPool::default();
        pool.put(1);
        pool.put(2);
        assert_eq!(pool.take(), Some(2));
    }

    #[test]
    fn a_hex_validator_round_trips() {
        let hex = "ab".repeat(32);
        assert_eq!(parse_hash(&Some(hex)).unwrap(), Some([0xab; 32]));
    }

    /// A quoted validator is what an HTTP client sends, ETags being quoted by
    /// specification. Refusing it would make the obvious client wrong.
    #[test]
    fn a_quoted_validator_is_accepted() {
        let quoted = format!("\"{}\"", "cd".repeat(32));
        assert_eq!(parse_hash(&Some(quoted)).unwrap(), Some([0xcd; 32]));
    }

    #[test]
    fn a_malformed_validator_is_refused_not_ignored() {
        assert!(parse_hash(&Some("nonsense".into())).is_err());
        assert!(parse_hash(&Some("zz".repeat(32))).is_err());
    }

    #[test]
    fn no_validator_is_not_an_error() {
        assert_eq!(parse_hash(&None).unwrap(), None);
    }

    /// The distinction the gateway exists not to launder: a `host:` name's
    /// address was read out of the name, and nothing signed it.
    #[test]
    fn only_an_address_resolution_is_unattested() {
        assert!(!is_attested(NameProvenance::Address));
        for p in [
            NameProvenance::Cache,
            NameProvenance::StaleCache,
            NameProvenance::Local,
            NameProvenance::Network,
        ] {
            assert!(is_attested(p), "{p:?} describes where, not whether");
        }
    }

    #[test]
    fn an_unresolvable_name_is_a_404_and_a_malformed_one_is_a_400() {
        let missing = into_response(NameResolution::NotFound {
            name: "spine://did:whatever/".into(),
        });
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let bad = into_response(NameResolution::Invalid {
            name: "not-a-name".into(),
            reason: "no scheme".into(),
        });
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    }

    /// A conditional hit has to carry its freshness in a header, because a 304
    /// has no body to put it in.
    #[test]
    fn an_unchanged_resolution_is_a_304_carrying_its_ttl() {
        let res = into_response(NameResolution::Unchanged { ttl_secs: 300 });
        assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            res.headers().get(header::CACHE_CONTROL).unwrap(),
            "max-age=300"
        );
    }

    #[test]
    fn a_resolution_reports_its_provenance_in_a_header() {
        let res = into_response(NameResolution::Resolved {
            record: serde_json::json!({}),
            provenance: NameProvenance::Network,
        });
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers().get("x-spine-provenance").unwrap(), "Network");
    }
}
