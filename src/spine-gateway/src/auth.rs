//! Bearer-token authentication middleware (secure by default).
//!
//! The gateway's OpenAPI schema declares a `bearer` security scheme;
//! this module is the actual enforcement.
//!
//! ## Startup contract
//!
//! As of v1.3.0 the gateway **refuses to start** without an explicit
//! choice about authentication. The deployer must set exactly one of:
//!
//! * `SPINE_GATEWAY_BEARER_TOKEN=<non-empty secret>` — turns auth on.
//!   Every request to every route (except `/health`, `/ready`, the
//!   Swagger UI, and `/api-docs`) must carry `Authorization: Bearer
//!   <secret>`. Comparison is constant-time via
//!   [`subtle::ConstantTimeEq`].
//!
//! * `SPINE_GATEWAY_ALLOW_UNAUTH=1` — explicitly opts OUT of
//!   authentication. The gateway logs a `WARN` and runs open. Use
//!   only for local development or when an upstream proxy
//!   authenticates.
//!
//! Setting neither is a misconfiguration and the gateway will exit
//! with a non-zero code at startup rather than silently expose
//! itself. This closes the v1.2.1 residual where bearer auth was
//! opt-in by default (CMMC AC.L1-3.1.1, MITRE T1190).
//!
//! ## Why an env var rather than a config field
//!
//! Bearer tokens are credentials and credentials should not live in
//! config files that get checked into version control. The env-var
//! channel matches the standard 12-factor pattern and is what
//! Kubernetes / Docker / systemd already wire up for secrets.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, Response, StatusCode};
use axum::middleware::Next;
use serde_json::json;
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// Loaded once at startup; `None` means "auth disabled".
#[derive(Clone)]
pub struct BearerConfig {
    expected: Arc<Vec<u8>>,
}

// Manual Debug — never print the secret bytes. Format prints the
// length only, which is enough for "is the token populated?" but
// leaks nothing useful to a screen-recording / log scrape.
impl std::fmt::Debug for BearerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BearerConfig")
            .field("token_len", &self.expected.len())
            .finish_non_exhaustive()
    }
}

/// Result of resolving the gateway's authentication mode at startup.
///
/// Exactly one variant is correct per deployment; the gateway binary
/// exits with a non-zero code if the env vars don't pick one.
#[derive(Debug)]
pub enum AuthMode {
    /// Bearer auth on — wrap the router with [`require_bearer`].
    Bearer(BearerConfig),
    /// Deployer explicitly opted out via `SPINE_GATEWAY_ALLOW_UNAUTH=1`.
    /// Gateway should run open; startup logs a `WARN`.
    Unauthenticated,
}

/// Error type for [`AuthMode::resolve`]. Carries the deployer-facing
/// message that the gateway prints before exiting.
#[derive(Debug, thiserror::Error)]
#[error(
    "spine-gateway requires an explicit authentication choice. Set either\n  \
     SPINE_GATEWAY_BEARER_TOKEN=<your-secret>  (turn auth on; recommended)\n  \
     SPINE_GATEWAY_ALLOW_UNAUTH=1               (explicit dev-mode opt-out)\n\
     before launching. Setting both is also rejected — pick one."
)]
pub struct AuthConfigError;

impl AuthMode {
    /// Read the env vars and pick a mode. Returns an error (which the
    /// caller logs and exits on) when:
    ///
    /// * neither var is set;
    /// * `SPINE_GATEWAY_BEARER_TOKEN` is set to an empty / whitespace
    ///   string;
    /// * both vars are set at the same time (ambiguous intent).
    pub fn resolve() -> Result<Self, AuthConfigError> {
        let token_raw = std::env::var("SPINE_GATEWAY_BEARER_TOKEN").ok();
        let allow_unauth = matches!(
            std::env::var("SPINE_GATEWAY_ALLOW_UNAUTH").ok().as_deref(),
            Some("1")
        );

        let token_present = token_raw
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

        match (token_present, allow_unauth) {
            (false, false) => Err(AuthConfigError),
            (true, true) => Err(AuthConfigError),
            (true, false) => {
                let trimmed = token_raw.expect("checked above").trim().to_string();
                Ok(AuthMode::Bearer(BearerConfig {
                    expected: Arc::new(trimmed.into_bytes()),
                }))
            }
            (false, true) => Ok(AuthMode::Unauthenticated),
        }
    }
}

impl BearerConfig {
    /// Read `SPINE_GATEWAY_BEARER_TOKEN` from the environment. Returns
    /// `None` (auth disabled) when the variable is unset or empty.
    ///
    /// Prefer [`AuthMode::resolve`] in new code — it enforces the
    /// secure-by-default startup contract.
    #[allow(dead_code)] // kept for backwards-compat with v1.2.x consumers
    pub fn from_env() -> Option<Self> {
        let raw = std::env::var("SPINE_GATEWAY_BEARER_TOKEN").ok()?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(Self {
            expected: Arc::new(trimmed.as_bytes().to_vec()),
        })
    }

    /// Build a config from an explicit token. Test-only — production
    /// must read the secret from the environment via [`Self::from_env`]
    /// so it isn't baked into the binary.
    #[cfg(test)]
    pub fn from_token(token: &str) -> Self {
        Self {
            expected: Arc::new(token.as_bytes().to_vec()),
        }
    }

    /// True when `presented` matches the configured secret. Constant
    /// time over the byte length to avoid leaking the prefix length via
    /// timing.
    fn matches(&self, presented: &[u8]) -> bool {
        // ConstantTimeEq requires equal-length operands; pad/truncate
        // by failing fast on length mismatch then ct-comparing the
        // common prefix. Length leak is bounded to "wrong-length or
        // not" which is the same as the comparison being constant-time.
        if presented.len() != self.expected.len() {
            return false;
        }
        presented.ct_eq(&self.expected).into()
    }
}

/// Axum middleware. Skips auth on a small allowlist of unauthenticated
/// paths (health probes, OpenAPI docs) and otherwise requires
/// `Authorization: Bearer <token>`.
pub async fn require_bearer(
    cfg: BearerConfig,
    req: Request,
    next: Next,
) -> Response<Body> {
    // Routes that must remain reachable without credentials so
    // kube probes, load balancers, and humans browsing the docs
    // can still function. Everything else requires the bearer.
    let path = req.uri().path();
    if is_unauthenticated_path(path) {
        return next.run(req).await;
    }

    let header_value = match req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        Some(v) => v,
        None => return unauthorized("missing Authorization header"),
    };

    let presented = match header_value.strip_prefix("Bearer ") {
        Some(t) => t.trim(),
        None => return unauthorized("Authorization header must use `Bearer` scheme"),
    };

    if !cfg.matches(presented.as_bytes()) {
        return unauthorized("invalid bearer token");
    }

    next.run(req).await
}

fn is_unauthenticated_path(path: &str) -> bool {
    matches!(path, "/health" | "/ready")
        || path.starts_with("/swagger-ui")
        || path.starts_with("/api-docs")
}

fn unauthorized(reason: &str) -> Response<Body> {
    let body = serde_json::to_vec(&json!({ "error": "unauthorized", "reason": reason }))
        .expect("static json is serializable");
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::WWW_AUTHENTICATE, "Bearer realm=\"spine-gateway\"")
        .body(Body::from(body))
        .expect("response is well-formed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// `cargo test` runs tests in parallel by default, so multiple
    /// tests that mutate the same env var race. Serialise just the
    /// env-touching cases through this mutex so one test sees a
    /// consistent view at a time. The non-env tests run in parallel.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn matches_constant_time_equal_strings() {
        let cfg = BearerConfig::from_token("hunter2");
        assert!(cfg.matches(b"hunter2"));
    }

    #[test]
    fn matches_rejects_wrong_token() {
        let cfg = BearerConfig::from_token("hunter2");
        assert!(!cfg.matches(b"wrong"));
        assert!(!cfg.matches(b""));
        assert!(!cfg.matches(b"hunter3"));
    }

    #[test]
    fn matches_rejects_length_mismatch() {
        // Different length must fail without comparing — but the
        // function must still return false, not panic.
        let cfg = BearerConfig::from_token("abc");
        assert!(!cfg.matches(b"abcd"));
        assert!(!cfg.matches(b"ab"));
    }

    #[test]
    fn from_env_returns_none_when_unset() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("SPINE_GATEWAY_BEARER_TOKEN");
        assert!(BearerConfig::from_env().is_none());
    }

    #[test]
    fn from_env_returns_none_when_empty() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("SPINE_GATEWAY_BEARER_TOKEN", "");
        assert!(BearerConfig::from_env().is_none());
        std::env::remove_var("SPINE_GATEWAY_BEARER_TOKEN");
    }

    #[test]
    fn from_env_returns_some_when_set() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("SPINE_GATEWAY_BEARER_TOKEN", "abc");
        let cfg = BearerConfig::from_env().expect("env var was just set");
        assert!(cfg.matches(b"abc"));
        std::env::remove_var("SPINE_GATEWAY_BEARER_TOKEN");
    }

    // -----------------------------------------------------------------
    // v1.3.0 — AuthMode::resolve secure-by-default startup contract.
    // -----------------------------------------------------------------

    /// Clear both env vars; called at the start of every AuthMode test
    /// so prior parallel tests can't bleed in.
    fn reset_env() {
        std::env::remove_var("SPINE_GATEWAY_BEARER_TOKEN");
        std::env::remove_var("SPINE_GATEWAY_ALLOW_UNAUTH");
    }

    #[test]
    fn resolve_rejects_neither_set() {
        let _g = ENV_LOCK.lock().unwrap();
        reset_env();
        let err = AuthMode::resolve().unwrap_err();
        // The Display impl is what the user sees on stderr; it must
        // mention both env var names so the deployer knows the fix.
        let msg = err.to_string();
        assert!(msg.contains("SPINE_GATEWAY_BEARER_TOKEN"));
        assert!(msg.contains("SPINE_GATEWAY_ALLOW_UNAUTH"));
    }

    #[test]
    fn resolve_rejects_both_set() {
        let _g = ENV_LOCK.lock().unwrap();
        reset_env();
        std::env::set_var("SPINE_GATEWAY_BEARER_TOKEN", "abc");
        std::env::set_var("SPINE_GATEWAY_ALLOW_UNAUTH", "1");
        assert!(AuthMode::resolve().is_err());
        reset_env();
    }

    #[test]
    fn resolve_rejects_empty_token() {
        let _g = ENV_LOCK.lock().unwrap();
        reset_env();
        std::env::set_var("SPINE_GATEWAY_BEARER_TOKEN", "   ");
        assert!(AuthMode::resolve().is_err());
        reset_env();
    }

    #[test]
    fn resolve_accepts_bearer_token() {
        let _g = ENV_LOCK.lock().unwrap();
        reset_env();
        std::env::set_var("SPINE_GATEWAY_BEARER_TOKEN", "hunter2");
        match AuthMode::resolve().expect("token set") {
            AuthMode::Bearer(cfg) => assert!(cfg.matches(b"hunter2")),
            other => panic!("expected Bearer, got {other:?}"),
        }
        reset_env();
    }

    #[test]
    fn resolve_accepts_explicit_unauth_optout() {
        let _g = ENV_LOCK.lock().unwrap();
        reset_env();
        std::env::set_var("SPINE_GATEWAY_ALLOW_UNAUTH", "1");
        match AuthMode::resolve().expect("opt-out set") {
            AuthMode::Unauthenticated => {}
            other => panic!("expected Unauthenticated, got {other:?}"),
        }
        reset_env();
    }

    #[test]
    fn resolve_rejects_unauth_other_than_1() {
        // Only the literal value "1" counts. Common typos / unset-style
        // values must be treated as "not set" and therefore fail the
        // neither-set check.
        for val in &["0", "true", "yes", "y", "TRUE"] {
            let _g = ENV_LOCK.lock().unwrap();
            reset_env();
            std::env::set_var("SPINE_GATEWAY_ALLOW_UNAUTH", val);
            let r = AuthMode::resolve();
            assert!(
                r.is_err(),
                "SPINE_GATEWAY_ALLOW_UNAUTH={val:?} should NOT enable unauth mode"
            );
            reset_env();
        }
    }

    #[test]
    fn unauthenticated_paths_skip_auth() {
        assert!(is_unauthenticated_path("/health"));
        assert!(is_unauthenticated_path("/ready"));
        assert!(is_unauthenticated_path("/swagger-ui/index.html"));
        assert!(is_unauthenticated_path("/api-docs/openapi.json"));
    }

    #[test]
    fn authenticated_paths_require_auth() {
        assert!(!is_unauthenticated_path("/api/sessions"));
        assert!(!is_unauthenticated_path("/v1/embeddings"));
        assert!(!is_unauthenticated_path("/v1/chat/completions"));
        assert!(!is_unauthenticated_path("/v1/agentic/capabilities"));
        assert!(!is_unauthenticated_path("/metrics"));
    }
}
